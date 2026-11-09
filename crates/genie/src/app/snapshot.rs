use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::BufReader;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::SystemTime;

use genie_core::{Locus, Region, Span, Target};
use genie_formats::bed;
use genie_formats::features::Entry;

use crate::export;
use crate::font;
use crate::genome;
use crate::location::Location;
use crate::paths::{self, file_name};
use crate::reference;
use crate::saved::Saved;
use crate::theme::Appearance;
use crate::tiles::TILING;
use crate::track;

use super::sessions::local;
use super::{Genie, Message};

pub(super) const USAGE: &str = "Usage: genie snapshot SESSION REGIONS FOLDER [--width POINTS] [--theme light|dark] [--format png|svg]";
const WIDTH: f32 = 1280.0;
const WIDTHS: RangeInclusive<f32> = 200.0..=4096.0;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Request {
    pub(super) session: PathBuf,
    pub(super) regions: PathBuf,
    pub(super) folder: PathBuf,
    pub(super) width: f32,
    pub(super) dark: bool,
    pub(super) svg: bool,
}

impl Request {
    pub(super) fn parse(arguments: &[OsString]) -> Result<Self, String> {
        let wrong = |message: &str| format!("{message}\n{USAGE}");
        let mut files = Vec::new();
        let (mut width, mut dark, mut svg) = (WIDTH, false, false);
        let mut rest = arguments.iter();
        while let Some(argument) = rest.next() {
            match argument.to_str() {
                Some("--width") => {
                    width = rest
                        .next()
                        .and_then(|value| value.to_str()?.parse::<f32>().ok())
                        .filter(|value| WIDTHS.contains(value))
                        .ok_or_else(|| wrong("The width must be a number from 200 to 4,096."))?;
                }
                Some("--theme") => {
                    dark = match rest.next().and_then(|value| value.to_str()) {
                        Some("light") => false,
                        Some("dark") => true,
                        _ => return Err(wrong("The theme must be light or dark.")),
                    };
                }
                Some("--format") => {
                    svg = match rest.next().and_then(|value| value.to_str()) {
                        Some("png") => false,
                        Some("svg") => true,
                        _ => return Err(wrong("The format must be png or svg.")),
                    };
                }
                Some(option) if option.starts_with("--") => {
                    return Err(wrong(&format!("genie does not know the option {option}.")));
                }
                _ => files.push(PathBuf::from(argument)),
            }
        }
        let [session, regions, folder] = <[PathBuf; 3]>::try_from(files)
            .map_err(|_| wrong("Give a session file, a BED file of regions and a folder."))?;
        Ok(Self {
            session,
            regions,
            folder,
            width,
            dark,
            svg,
        })
    }
}

pub fn command(arguments: &[OsString]) -> ExitCode {
    let request = match Request::parse(arguments) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    font::load();
    match run(&request, &mut |line| println!("{line}")) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

pub(super) fn image_names(entries: &[Entry], extension: &str) -> Vec<String> {
    let mut used = HashSet::new();
    entries
        .iter()
        .map(|entry| {
            let stem = if entry.name.is_empty() {
                export::stem(&entry.contig, entry.span)
            } else {
                export::safe(&entry.name)
            };
            let mut name = format!("{stem}.{extension}");
            let mut copy = 1;
            while !used.insert(name.to_lowercase()) {
                copy += 1;
                name = format!("{stem}-{copy}.{extension}");
            }
            name
        })
        .collect()
}

pub(super) fn run(request: &Request, report: &mut dyn FnMut(String)) -> Result<usize, String> {
    let session = file_name(&request.session);
    let saved = Saved::load(&request.session)
        .map_err(|error| format!("genie cannot open the session {session}. {error}"))?;
    let regions = read_regions(&request.regions)?;
    let mut genie = Genie::default();
    genie.settings.motion = false;
    genie.settings.appearance = if request.dark {
        Appearance::Dark
    } else {
        Appearance::Light
    };
    genie.open_now(saved, report)?;
    let _ = genie.update(Message::Resized(request.width));
    fs::create_dir_all(&request.folder).map_err(|error| {
        format!(
            "genie cannot make the folder {}. {error}",
            request.folder.display()
        )
    })?;
    let count = regions.len();
    let mut failed = 0;
    let extension = if request.svg { "svg" } else { "png" };
    let names = image_names(&regions, extension);
    for (index, (entry, name)) in regions.iter().zip(names).enumerate() {
        let number = index + 1;
        let path = request.folder.join(&name);
        let result = genie
            .visit(entry)
            .and_then(|()| genie.save(path, request.svg));
        match result {
            Ok(()) => report(format!("Saved {name} ({number} of {count}).")),
            Err(error) => {
                failed += 1;
                let locus = Locus {
                    name: entry.contig.clone(),
                    span: Span::Region(entry.span),
                };
                report(format!("No image for region {number} ({locus}). {error}"));
            }
        }
    }
    Ok(failed)
}

fn read_regions(path: &Path) -> Result<Vec<Entry>, String> {
    let name = file_name(path);
    let file = File::open(path).map_err(|error| format!("genie cannot open {name}. {error}"))?;
    bed::regions(BufReader::new(file))
        .map_err(|error| format!("The file {name} is not valid. {error}"))
}

impl Genie {
    fn open_now(&mut self, saved: Saved, report: &mut dyn FnMut(String)) -> Result<(), String> {
        let genome = saved.genome.as_deref().and_then(genome::find);
        let folder = genome
            .as_ref()
            .zip(self.cache.as_ref())
            .map(|(genome, cache)| genome.folder(cache));
        let tracks: Vec<Location> = saved
            .tracks
            .iter()
            .map(|track| local(&track.location, genome.as_ref(), folder.as_deref()))
            .collect();
        let reference = match (&genome, &folder) {
            (Some(genome), Some(folder)) => genome::open(genome, folder, SystemTime::now())?,
            _ => reference::open(
                saved.reference.clone(),
                paths::cache_dir().as_deref(),
                &mut |_| {},
            )?,
        };
        self.pending.clone_from(&tracks);
        self.restoring = Some(saved);
        let _ = self.update(Message::Opened(Ok(Arc::new(reference))));
        for location in tracks {
            let name = location.name();
            let result = track::open(location).map(Arc::new);
            if let Err(error) = &result {
                report(format!("genie cannot open the track {name}. {error}"));
            }
            let _ = self.update(Message::TrackOpened(result));
        }
        Ok(())
    }

    fn visit(&mut self, entry: &Entry) -> Result<(), String> {
        let contig = self
            .session
            .as_ref()
            .and_then(|session| session.reference.assembly.find(&entry.contig))
            .ok_or_else(|| format!("The reference has no contig {}.", entry.contig))?;
        let _ = self.go_to(Target {
            contig,
            span: Span::Region(entry.span),
        });
        self.load_now();
        Ok(())
    }

    pub(super) fn save(&self, path: PathBuf, svg: bool) -> Result<(), String> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| "There is no reference.".to_owned())?;
        if svg {
            return fs::write(path, self.figure(session))
                .map_err(|error| format!("The image could not be saved. {error}"));
        }
        let image = export::render(self.export_view(session), self.width, &self.theme())
            .ok_or_else(|| "The view could not be drawn.".to_owned())?;
        export::save(path, &image).map(|_| ())
    }

    fn load_now(&mut self) {
        let Some(session) = &self.session else {
            return;
        };
        let (contig, viewport) = (session.contig, session.viewport);
        let reference = Arc::clone(&session.reference);
        let visible = viewport.visible();
        let (step, detail) = (track::step(viewport.scale()), track::detailed(visible));
        let jobs: Vec<_> = session
            .tracks
            .iter()
            .filter_map(|track| {
                let (contig, region) = track.slot.wanted?;
                let name = track.contigs[contig].clone()?;
                let source = Arc::clone(&track.source);
                Some((track.id, source, name, contig, region, track.options))
            })
            .collect();
        if viewport.scale() <= 1.0 {
            self.tiles.clear();
            let wanted = visible.expand(visible.len(), viewport.length());
            for key in self.tiles.request(contig, wanted) {
                let bases = reference.fetch(key.contig, TILING.span(key.index));
                self.tile_loaded(self.generation, key, bases.map(Arc::from));
            }
        }
        let length = viewport.length();
        for (id, source, name, contig, region, options) in jobs {
            let bases = |window: Region| reference.fetch(contig, window.clamp_to(length));
            let result =
                track::load(&source, &name, region, options, step, detail, bases).map(Arc::new);
            self.on_session(|session| session.accept(id, contig, region, result));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;

    use genie_core::Region;
    use genie_formats::features::Entry;

    use super::{Request, USAGE, image_names, run};
    use crate::scratch::Scratch;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn requests_take_a_session_regions_a_folder_and_options() {
        let request = Request::parse(&args(&["work.genie", "regions.bed", "out"])).unwrap();
        assert_eq!(
            request,
            Request {
                session: PathBuf::from("work.genie"),
                regions: PathBuf::from("regions.bed"),
                folder: PathBuf::from("out"),
                width: 1280.0,
                dark: false,
                svg: false,
            }
        );
        let words = [
            "--width",
            "800",
            "work.genie",
            "--theme",
            "dark",
            "regions.bed",
            "--format",
            "svg",
            "out",
        ];
        let request = Request::parse(&args(&words)).unwrap();
        assert_eq!(
            (request.width, request.dark, request.svg),
            (800.0, true, true)
        );
        let wrong: [&[&str]; 7] = [
            &["a", "b", "c", "--format", "gif"],
            &["work.genie", "regions.bed"],
            &["a", "b", "c", "d"],
            &["a", "b", "c", "--width"],
            &["a", "b", "c", "--width", "50"],
            &["a", "b", "c", "--theme", "blue"],
            &["a", "b", "c", "--size", "9"],
        ];
        for words in wrong {
            let error = Request::parse(&args(words)).unwrap_err();
            assert!(error.ends_with(USAGE), "{error}");
        }
    }

    fn entry(name: &str, contig: &str, start: u64, end: u64) -> Entry {
        Entry {
            name: name.to_owned(),
            contig: contig.to_owned(),
            span: Region::between(start, end),
        }
    }

    #[test]
    fn images_take_the_region_name_or_the_locus() {
        let entries = [
            entry("BRCA1", "chr17", 10, 20),
            entry("", "chr1", 99, 200),
            entry("BRCA1", "chr17", 30, 40),
            entry("a/b c", "chr2", 0, 5),
        ];
        assert_eq!(
            image_names(&entries, "png"),
            ["BRCA1.png", "chr1_100-200.png", "BRCA1-2.png", "a_b_c.png"]
        );
        assert_eq!(image_names(&entries[..1], "svg"), ["BRCA1.svg"]);
    }

    #[test]
    fn snapshots_save_one_image_per_region() {
        let scratch = Scratch::new("snapshot");
        let bases = "ACGT".repeat(250);
        scratch.file("ref.fa", format!(">chr1\n{bases}\n"));
        scratch.file("genes.bed", "chr1\t100\t400\tgeneA\t0\t+\n");
        let session = scratch.file(
            "work.genie",
            "[session]\nreference = ref.fa\nlocus = chr1:1-1000\n[track]\nfile = genes.bed\n",
        );
        let regions = scratch.file(
            "regions.bed",
            "chr1\t100\t400\tgeneA\nchr1\t0\t50\nchrZ\t0\t10\tlost\n",
        );
        let folder = session.with_file_name("pictures");
        let request = Request {
            session,
            regions,
            folder: folder.clone(),
            width: 400.0,
            dark: true,
            svg: false,
        };
        let mut lines = Vec::new();
        let failed = run(&request, &mut |line| lines.push(line)).unwrap();
        assert_eq!(failed, 1);
        assert_eq!(
            lines,
            [
                "Saved geneA.png (1 of 3).",
                "Saved chr1_1-50.png (2 of 3).",
                "No image for region 3 (chrZ:1-10). The reference has no contig chrZ."
            ]
        );
        let png = fs::read(folder.join("geneA.png")).unwrap();
        assert_eq!(png[16..20], 800_u32.to_be_bytes());
        assert!(folder.join("chr1_1-50.png").is_file());
        let request = Request {
            svg: true,
            ..request
        };
        assert_eq!(run(&request, &mut |_| {}), Ok(1));
        let svg = fs::read_to_string(folder.join("geneA.svg")).unwrap();
        assert!(
            svg.starts_with("<svg") && svg.contains(">geneA</text>"),
            "{svg}"
        );
    }

    #[test]
    fn missing_files_stop_the_snapshots() {
        let scratch = Scratch::new("snapshot-missing");
        let session = scratch.file("work.genie", "[session]\nreference = gone.fa\n");
        let regions = scratch.file("regions.bed", "chr1\t0\t10\n");
        let request = Request {
            session: session.clone(),
            regions,
            folder: session.with_file_name("out"),
            width: 400.0,
            dark: false,
            svg: false,
        };
        assert!(run(&request, &mut |_| {}).is_err());
        let request = Request {
            regions: session.with_file_name("none.bed"),
            ..request
        };
        assert!(run(&request, &mut |_| {}).is_err());
    }
}
