use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use genie_formats::fields::{self, Rule, Test};
use genie_formats::http::Url;
use genie_formats::ini::{self, Document, Section};
use genie_render::alignments::Look;
use genie_render::features::Mode;
use genie_render::scene::Hue;
use genie_render::signal::{Scale, Style};

use crate::location::Location;
use crate::reading::{Frames, Reading};
use crate::track::{Filter, Grouping, Options, Plot};

pub const EXTENSION: &str = "genie";
const MODES: [(&str, Mode); 3] = [
    ("expanded", Mode::Expanded),
    ("squished", Mode::Squished),
    ("collapsed", Mode::Collapsed),
];
const GROUPS: [(&str, Grouping); 2] = [("phase", Grouping::Phase), ("strand", Grouping::Strand)];
const HUES: [(&str, Hue); 5] = [
    ("blue", Hue::Blue),
    ("green", Hue::Green),
    ("orange", Hue::Orange),
    ("vermillion", Hue::Vermillion),
    ("purple", Hue::Purple),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub reference: Location,
    pub genome: Option<String>,
    pub locus: Option<String>,
    pub reading: Reading,
    pub tracks: Vec<SavedTrack>,
    pub regions: Vec<SavedRegion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedRegion {
    pub name: String,
    pub locus: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SavedTrack {
    pub location: Location,
    pub name: Option<String>,
    pub over: Option<usize>,
    pub mode: Option<Mode>,
    pub hue: Hue,
    pub height: Option<f32>,
    pub options: Options,
    pub plot: Plot,
    pub filter: Filter,
}

impl SavedTrack {
    pub fn plain(location: Location) -> Self {
        Self {
            location,
            name: None,
            over: None,
            mode: None,
            hue: Hue::default(),
            height: None,
            options: Options::default(),
            plot: Plot::default(),
            filter: Filter::default(),
        }
    }
}

impl Saved {
    pub fn parse(text: &str, base: &Path) -> Result<Self, String> {
        let document =
            ini::parse(text).map_err(|error| format!("The session file is not valid. {error}"))?;
        let session = document.section("session");
        let value = |key| session.and_then(|section| section.value(key));
        let reference = value("reference")
            .ok_or_else(|| "The session file has no reference sequence.".to_owned())?;
        let reading = Reading {
            reverse: value("strand") == Some("reverse"),
            frames: match value("frames") {
                Some("3") => Frames::Three,
                Some("6") => Frames::Six,
                _ => Frames::Off,
            },
        };
        let tracks = document
            .sections("track")
            .filter_map(|section| {
                let on = |key| section.value(key) == Some("on");
                Some(SavedTrack {
                    location: read(section.value("file")?, base),
                    name: section.value("name").map(str::to_owned),
                    over: section
                        .value("overlay")
                        .and_then(|text| text.parse::<usize>().ok())
                        .and_then(|number| number.checked_sub(1)),
                    mode: section.value("mode").and_then(|name| find(&MODES, name)),
                    hue: section
                        .value("color")
                        .and_then(|name| find(&HUES, name))
                        .unwrap_or_default(),
                    height: section.value("height").and_then(|text| text.parse().ok()),
                    options: Options {
                        duplicates: on("duplicates"),
                        clips: on("clips"),
                        downsample: section.value("downsample") != Some("off"),
                        group: section
                            .value("group")
                            .and_then(|name| find(&GROUPS, name))
                            .unwrap_or_default(),
                    },
                    plot: Plot {
                        style: if section.value("style") == Some("line") {
                            Style::Line
                        } else {
                            Style::Bars
                        },
                        linked: section.value("scale") == Some("shared"),
                        look: Look {
                            pairs: section.value("pairs") != Some("off"),
                            mods: section.value("modifications") == Some("on"),
                        },
                        colors: section.value("colors") != Some("off"),
                        range: section.value("range").and_then(range),
                    },
                    filter: Filter {
                        rules: section
                            .entries
                            .iter()
                            .filter_map(|(key, value)| rule(key, value))
                            .collect(),
                        label: section.value("label").map(str::to_owned),
                    },
                })
            })
            .collect();
        let regions = document
            .sections("region")
            .filter_map(|section| {
                Some(SavedRegion {
                    locus: section.value("locus")?.to_owned(),
                    name: section.value("name").unwrap_or_default().to_owned(),
                })
            })
            .collect();
        Ok(Self {
            reference: read(reference, base),
            genome: value("genome").map(str::to_owned),
            locus: value("locus").map(str::to_owned),
            reading,
            tracks,
            regions,
        })
    }

    pub fn text(&self, base: &Path) -> String {
        let mut session = vec![("reference", written(&self.reference, base))];
        if let Some(genome) = &self.genome {
            session.push(("genome", genome.clone()));
        }
        if let Some(locus) = &self.locus {
            session.push(("locus", locus.clone()));
        }
        if self.reading.reverse {
            session.push(("strand", "reverse".to_owned()));
        }
        match self.reading.frames {
            Frames::Off => {}
            Frames::Three => session.push(("frames", "3".to_owned())),
            Frames::Six => session.push(("frames", "6".to_owned())),
        }
        let mut sections = vec![section("session", session)];
        for track in &self.tracks {
            let mut entries = vec![("file", written(&track.location, base))];
            if let Some(name) = &track.name {
                entries.push(("name", name.clone()));
            }
            if let Some(over) = track.over {
                entries.push(("overlay", (over + 1).to_string()));
            }
            if let Some(mode) = track.mode {
                entries.push(("mode", name(&MODES, mode).to_owned()));
            }
            entries.push(("color", name(&HUES, track.hue).to_owned()));
            if let Some(height) = track.height {
                entries.push(("height", height.to_string()));
            }
            if track.options.duplicates {
                entries.push(("duplicates", "on".to_owned()));
            }
            if track.options.clips {
                entries.push(("clips", "on".to_owned()));
            }
            if !track.options.downsample {
                entries.push(("downsample", "off".to_owned()));
            }
            if track.options.group != Grouping::None {
                entries.push(("group", name(&GROUPS, track.options.group).to_owned()));
            }
            if track.plot.style == Style::Line {
                entries.push(("style", "line".to_owned()));
            }
            if track.plot.linked {
                entries.push(("scale", "shared".to_owned()));
            }
            if let Some(range) = track.plot.range {
                entries.push(("range", format!("{} {}", range.min, range.max)));
            }
            if !track.plot.look.pairs {
                entries.push(("pairs", "off".to_owned()));
            }
            if track.plot.look.mods {
                entries.push(("modifications", "on".to_owned()));
            }
            if !track.plot.colors {
                entries.push(("colors", "off".to_owned()));
            }
            for rule in &track.filter.rules {
                let (key, value) = match &rule.test {
                    Test::Keep(value) => ("keep", value.clone()),
                    Test::Hide(value) => ("hide", value.clone()),
                    Test::AtLeast(bound) => ("least", bound.to_string()),
                    Test::AtMost(bound) => ("most", bound.to_string()),
                };
                entries.push((key, format!("{}: {value}", rule.label)));
            }
            if let Some(label) = &track.filter.label {
                entries.push(("label", label.clone()));
            }
            sections.push(section("track", entries));
        }
        for region in &self.regions {
            let entries = vec![
                ("locus", region.locus.clone()),
                ("name", region.name.clone()),
            ];
            sections.push(section("region", entries));
        }
        ini::write(&Document { sections })
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
        Self::parse(&text, path.parent().unwrap_or(Path::new("")))
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        crate::paths::replace(path, &self.text(path.parent().unwrap_or(Path::new(""))))
    }
}

pub fn is_session(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION))
}

fn section(name: &str, entries: Vec<(&str, String)>) -> Section {
    Section {
        name: name.to_owned(),
        entries: entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    }
}

fn read(text: &str, base: &Path) -> Location {
    if Url::is_url(text)
        && let Ok(url) = Url::parse(text)
    {
        return Location::Url(url);
    }
    Location::File(absolute(text, base))
}

fn written(location: &Location, base: &Path) -> String {
    match location {
        Location::File(path) => relative(path, base),
        Location::Url(url) => url.to_string(),
    }
}

fn relative(path: &Path, base: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(inside) => inside
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => path.display().to_string(),
    }
}

fn absolute(text: &str, base: &Path) -> PathBuf {
    let path = PathBuf::from(text);
    if path.is_absolute() || path.has_root() {
        path
    } else {
        base.join(path)
    }
}

fn name<T: Copy + PartialEq>(table: &[(&'static str, T)], value: T) -> &'static str {
    table
        .iter()
        .find(|(_, known)| *known == value)
        .map_or("", |(name, _)| name)
}

fn find<T: Copy>(table: &[(&str, T)], name: &str) -> Option<T> {
    table
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, value)| *value)
}

fn rule(key: &str, entry: &str) -> Option<Rule> {
    let (label, value) = entry.split_once(':')?;
    let value = value.trim();
    let test = match key {
        "keep" => Test::Keep(value.to_owned()),
        "hide" => Test::Hide(value.to_owned()),
        "least" => Test::AtLeast(fields::number(value)?),
        "most" => Test::AtMost(fields::number(value)?),
        _ => return None,
    };
    Some(Rule {
        label: label.trim().to_owned(),
        test,
    })
}

fn range(text: &str) -> Option<Scale> {
    let mut numbers = text
        .split_whitespace()
        .map(|word| word.parse::<f32>().ok().filter(|value| value.is_finite()));
    let (Some(Some(min)), Some(Some(max)), None) = (numbers.next(), numbers.next(), numbers.next())
    else {
        return None;
    };
    (max > min).then_some(Scale { min, max })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use genie_render::alignments::Look;
    use genie_render::features::Mode;
    use genie_render::scene::Hue;
    use genie_render::signal::{Scale, Style};

    use genie_formats::fields::{Rule, Test};

    use super::{Saved, SavedRegion, SavedTrack, is_session};
    use crate::location::Location;
    use crate::reading::{Frames, Reading};
    use crate::track::{Filter, Grouping, Options, Plot};

    fn base() -> PathBuf {
        PathBuf::from("/work/project")
    }

    fn rule(label: &str, test: Test) -> Rule {
        Rule {
            label: label.to_owned(),
            test,
        }
    }

    #[test]
    fn tracks_keep_their_filter_and_label() {
        let text = saved().text(&base());
        for line in [
            "keep = Clinical significance: Pathogenic",
            "least = Minor allele frequency: 0.05",
            "hide = Gene type: lncRNA",
            "most = Quality: 30",
            "label = Gene symbol",
        ] {
            assert!(text.contains(line), "{line}\n{text}");
        }
        assert_eq!(text.matches("keep =").count(), 1, "{text}");
    }

    fn saved() -> Saved {
        Saved {
            reference: Location::File(base().join("data").join("ref.fa.gz")),
            genome: None,
            locus: Some("chr1:51,971-52,030".to_owned()),
            reading: Reading {
                reverse: true,
                frames: Frames::Six,
            },
            tracks: vec![
                SavedTrack {
                    location: Location::File(base().join("reads.bam")),
                    name: Some("Tumor A".to_owned()),
                    over: None,
                    mode: Some(Mode::Squished),
                    hue: Hue::Blue,
                    height: Some(90.0),
                    options: Options {
                        duplicates: true,
                        clips: true,
                        downsample: false,
                        group: Grouping::Phase,
                    },
                    plot: Plot {
                        look: Look {
                            pairs: false,
                            mods: true,
                        },
                        ..Plot::default()
                    },
                    filter: Filter::default(),
                },
                SavedTrack {
                    location: Location::File(PathBuf::from("/elsewhere/genes.bed.gz")),
                    name: None,
                    over: None,
                    mode: None,
                    hue: Hue::Vermillion,
                    height: None,
                    options: Options::default(),
                    plot: Plot::default(),
                    filter: Filter {
                        rules: vec![
                            rule("Clinical significance", Test::Keep("Pathogenic".to_owned())),
                            rule("Minor allele frequency", Test::AtLeast(0.05)),
                            rule("Gene type", Test::Hide("lncRNA".to_owned())),
                            rule("Quality", Test::AtMost(30.0)),
                        ],
                        label: Some("Gene symbol".to_owned()),
                    },
                },
                SavedTrack {
                    location: Location::File(base().join("coverage.bw")),
                    name: None,
                    over: Some(0),
                    mode: None,
                    hue: Hue::Green,
                    height: Some(64.0),
                    options: Options::default(),
                    plot: Plot {
                        style: Style::Line,
                        linked: true,
                        colors: false,
                        range: Some(Scale {
                            min: -1.5,
                            max: 50.0,
                        }),
                        ..Plot::default()
                    },
                    filter: Filter::default(),
                },
            ],
            regions: vec![
                SavedRegion {
                    name: "promoter A".to_owned(),
                    locus: "chr1:1,201-1,450".to_owned(),
                },
                SavedRegion {
                    name: "Region 2".to_owned(),
                    locus: "chr2:11-20".to_owned(),
                },
            ],
        }
    }

    #[test]
    fn sessions_keep_addresses_as_written() {
        let mut saved = saved();
        saved.reference = Location::parse("https://example.org/hg38.2bit").unwrap();
        saved.tracks[0].location = Location::parse("https://example.org/a.bam?k=1").unwrap();
        let text = saved.text(&base());
        assert!(
            text.contains("reference = https://example.org/hg38.2bit"),
            "{text}"
        );
        assert!(
            text.contains("file = https://example.org/a.bam?k=1"),
            "{text}"
        );
        assert_eq!(Saved::parse(&text, &base()), Ok(saved));
    }

    #[test]
    fn sessions_keep_the_genome() {
        let mut saved = saved();
        saved.genome = Some("hg38".to_owned());
        let text = saved.text(&base());
        assert!(text.contains("genome = hg38"), "{text}");
        assert_eq!(Saved::parse(&text, &base()), Ok(saved));
        assert!(!self::saved().text(&base()).contains("genome ="));
    }

    #[test]
    fn regions_are_sections_with_a_locus_and_a_name() {
        let text = saved().text(&base());
        assert_eq!(text.matches("[region]").count(), 2, "{text}");
        assert!(text.contains("locus = chr1:1,201-1,450"), "{text}");
        assert!(text.contains("name = promoter A"), "{text}");
    }

    #[test]
    fn tracks_keep_a_new_name() {
        let text = saved().text(&base());
        assert!(text.contains("name = Tumor A"), "{text}");
        assert_eq!(text.matches("name = ").count(), 3, "{text}");
    }

    #[test]
    fn sessions_read_back_what_they_write() {
        let saved = saved();
        assert_eq!(Saved::parse(&saved.text(&base()), &base()), Ok(saved));
    }

    #[test]
    fn signal_tracks_keep_their_style_and_shared_scale() {
        let text = saved().text(&base());
        assert!(text.contains("style = line"), "{text}");
        assert!(text.contains("scale = shared"), "{text}");
        assert!(text.contains("range = -1.5 50"), "{text}");
        assert!(text.contains("overlay = 1"), "{text}");
        assert_eq!(text.matches("style =").count(), 1, "{text}");
        assert_eq!(text.matches("pairs = off").count(), 1, "{text}");
        assert_eq!(text.matches("modifications = on").count(), 1, "{text}");
        assert_eq!(text.matches("group = phase").count(), 1, "{text}");
        assert_eq!(text.matches("colors = off").count(), 1, "{text}");
    }

    #[test]
    fn files_beside_the_session_are_relative() {
        let text = saved().text(&base());
        assert!(text.contains("reference = data/ref.fa.gz"), "{text}");
        assert!(text.contains("file = reads.bam"), "{text}");
        assert!(text.contains("file = /elsewhere/genes.bed.gz"), "{text}");
        let moved = Saved::parse(&text, Path::new("/moved")).unwrap();
        let moved_reference = Location::File(Path::new("/moved").join("data/ref.fa.gz"));
        assert_eq!(moved.reference, moved_reference);
    }

    #[test]
    fn sessions_need_a_reference() {
        let error = Saved::parse("[session]\nlocus = chr1\n", &base()).unwrap_err();
        assert_eq!(error, "The session file has no reference sequence.");
    }

    #[test]
    fn knows_session_files_by_name() {
        assert!(is_session(Path::new("/a/b.genie")));
        assert!(is_session(Path::new("B.GENIE")));
        assert!(!is_session(Path::new("b.fa")));
    }
}
