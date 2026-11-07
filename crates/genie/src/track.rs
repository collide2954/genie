use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use genie_core::{Assembly, Feature, Region};
use genie_formats::bgzf::Bgzf;
use genie_formats::features::{self, Kind};
use genie_formats::tabix::Index;
use genie_render::features::{Mode, pack};
use genie_render::scene::Hue;

use crate::paths::file_name;

#[derive(Debug)]
pub struct Source {
    pub path: PathBuf,
    pub name: String,
    pub kind: Kind,
    pub index: Index,
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

#[derive(Debug, PartialEq)]
pub struct Loaded {
    pub region: Region,
    pub features: Vec<Feature>,
    pub rows: Vec<usize>,
    pub count: usize,
}

impl Loaded {
    pub fn new(region: Region, features: Vec<Feature>) -> Self {
        let rows = pack(&features, 0);
        let count = rows.iter().max().map_or(0, |row| row + 1);
        Self {
            region,
            features,
            rows,
            count,
        }
    }
}

#[derive(Debug)]
pub struct Track {
    pub id: u64,
    pub source: Arc<Source>,
    pub names: Vec<Option<String>>,
    pub hue: Hue,
    pub mode: Option<Mode>,
    pub loaded: Option<(usize, Arc<Loaded>)>,
    pub wanted: Option<(usize, Region)>,
    pub error: Option<String>,
}

impl Track {
    pub fn new(id: u64, source: Arc<Source>, assembly: &Assembly) -> Self {
        let names = contig_names(source.index.names(), assembly);
        Self {
            id,
            source,
            names,
            hue: Hue::default(),
            mode: None,
            loaded: None,
            wanted: None,
            error: None,
        }
    }

    pub fn data(&self, contig: usize) -> Option<&Loaded> {
        self.loaded
            .as_ref()
            .filter(|(loaded, _)| *loaded == contig)
            .map(|(_, data)| data.as_ref())
    }

    pub fn mode(&self, contig: usize) -> Mode {
        self.mode
            .unwrap_or_else(|| Mode::fit(self.data(contig).map_or(0, |data| data.count)))
    }

    pub fn needs(&self, contig: usize, visible: Region) -> bool {
        let covers = |(wanted, region): (usize, Region)| {
            wanted == contig && region.start() <= visible.start() && visible.end() <= region.end()
        };
        let loaded = self.data(contig).map(|data| (contig, data.region));
        self.names.get(contig).is_some_and(Option::is_some)
            && !loaded.is_some_and(covers)
            && !self.wanted.is_some_and(covers)
    }

    pub fn accept(&mut self, contig: usize, region: Region, result: Result<Arc<Loaded>, String>) {
        if self.wanted != Some((contig, region)) {
            return;
        }
        self.wanted = None;
        match result {
            Ok(data) => {
                self.loaded = Some((contig, data));
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Mode(Mode),
    Hue(Hue),
    Up,
    Down,
    Remove,
}

pub fn change(tracks: &mut Vec<Track>, id: u64, change: Change) {
    let Some(position) = tracks.iter().position(|track| track.id == id) else {
        return;
    };
    match change {
        Change::Mode(mode) => tracks[position].mode = Some(mode),
        Change::Hue(hue) => tracks[position].hue = hue,
        Change::Up if position > 0 => tracks.swap(position, position - 1),
        Change::Down if position + 1 < tracks.len() => tracks.swap(position, position + 1),
        Change::Up | Change::Down => {}
        Change::Remove => {
            tracks.remove(position);
        }
    }
}

pub fn contig_names(names: &[String], assembly: &Assembly) -> Vec<Option<String>> {
    let mut found = vec![None; assembly.contigs().len()];
    for name in names {
        if let Some(contig) = assembly.find(name) {
            found[contig].get_or_insert_with(|| name.clone());
        }
    }
    found
}

pub fn is_track(path: &Path) -> bool {
    Kind::of(&file_name(path)).is_some()
}

pub fn open(path: PathBuf) -> Result<Source, String> {
    let name = file_name(&path);
    let kind =
        Kind::of(&name).ok_or_else(|| format!("The file {name} is not a BED or GFF3 file."))?;
    let compressed = path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("gz") || extension.eq_ignore_ascii_case("bgz")
    });
    if !compressed {
        return Err(format!(
            "Compress {name} with bgzip, then index it with tabix."
        ));
    }
    let mut index = path.clone().into_os_string();
    index.push(".tbi");
    let file = File::open(&index)
        .map_err(|_| format!("There is no index {name}.tbi. Index the file with tabix."))?;
    let index =
        Index::read(file).map_err(|error| format!("The index {name}.tbi is not valid. {error}"))?;
    Ok(Source {
        path,
        name,
        kind,
        index,
    })
}

pub fn load(source: &Source, contig: &str, region: Region) -> Result<Loaded, String> {
    let file = File::open(&source.path).map_err(|error| error.to_string())?;
    let mut reader = Bgzf::new(file);
    let (region, features) =
        features::query(source.kind, &source.index, &mut reader, contig, region)
            .map_err(|error| error.to_string())?;
    Ok(Loaded::new(region, features))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::{Assembly, Contig, Feature, Region, Strand};
    use genie_formats::features::Kind;
    use genie_formats::tabix::Index;
    use genie_render::features::Mode;
    use genie_render::scene::Hue;

    use super::{Change, Loaded, Source, Track, change, contig_names, open};
    use crate::scratch::Scratch;

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    fn assembly() -> Assembly {
        Assembly::new(
            ["chr1", "chr2", "chrM"]
                .into_iter()
                .map(|name| Contig {
                    name: name.to_owned(),
                    length: 1_000_000,
                })
                .collect(),
        )
    }

    fn track() -> Track {
        let source = Source {
            path: PathBuf::from("/data/genes.bed.gz"),
            name: "genes.bed.gz".to_owned(),
            kind: Kind::Bed,
            index: Index::default(),
        };
        let mut track = Track::new(7, Arc::new(source), &assembly());
        track.names = vec![Some("1".to_owned()), None, None];
        track
    }

    fn loaded(region: Region, count: usize) -> Arc<Loaded> {
        let features = (0..count)
            .map(|_| Feature::new("", region, Strand::Forward))
            .collect();
        Arc::new(Loaded::new(region, features))
    }

    fn ids(tracks: &[Track]) -> Vec<u64> {
        tracks.iter().map(|track| track.id).collect()
    }

    fn three() -> Vec<Track> {
        (0..3)
            .map(|id| {
                let mut track = track();
                track.id = id;
                track
            })
            .collect()
    }

    #[test]
    fn changes_set_the_mode_and_the_hue() {
        let mut tracks = three();
        change(&mut tracks, 1, Change::Mode(Mode::Collapsed));
        change(&mut tracks, 1, Change::Hue(Hue::Orange));
        assert_eq!(tracks[1].mode, Some(Mode::Collapsed));
        assert_eq!(tracks[1].hue, Hue::Orange);
    }

    #[test]
    fn changes_move_tracks_inside_the_list() {
        let mut tracks = three();
        change(&mut tracks, 0, Change::Up);
        assert_eq!(ids(&tracks), [0, 1, 2]);
        change(&mut tracks, 0, Change::Down);
        assert_eq!(ids(&tracks), [1, 0, 2]);
        change(&mut tracks, 2, Change::Up);
        assert_eq!(ids(&tracks), [1, 2, 0]);
        change(&mut tracks, 0, Change::Down);
        assert_eq!(ids(&tracks), [1, 2, 0]);
    }

    #[test]
    fn changes_remove_tracks() {
        let mut tracks = three();
        change(&mut tracks, 1, Change::Remove);
        change(&mut tracks, 9, Change::Remove);
        assert_eq!(ids(&tracks), [0, 2]);
    }

    #[test]
    fn maps_track_contigs_to_reference_contigs() {
        let names = ["1".to_owned(), "MT".to_owned(), "chrUn".to_owned()];
        assert_eq!(
            contig_names(&names, &assembly()),
            [Some("1".to_owned()), None, Some("MT".to_owned())]
        );
    }

    #[test]
    fn needs_data_until_a_request_covers_the_view() {
        let mut track = track();
        assert!(track.needs(0, region(100, 200)));
        assert!(!track.needs(1, region(100, 200)));
        track.wanted = Some((0, region(0, 300)));
        assert!(!track.needs(0, region(100, 200)));
        assert!(track.needs(0, region(250, 350)));
    }

    #[test]
    fn accepts_only_the_latest_request() {
        let mut track = track();
        track.wanted = Some((0, region(0, 300)));
        track.accept(0, region(0, 100), Ok(loaded(region(0, 100), 1)));
        assert!(track.data(0).is_none());
        track.accept(0, region(0, 300), Ok(loaded(region(0, 300), 1)));
        assert_eq!(track.data(0).unwrap().region, region(0, 300));
        assert!(!track.needs(0, region(100, 200)));
        assert!(track.data(1).is_none());
    }

    #[test]
    fn keeps_errors_for_the_view() {
        let mut track = track();
        track.wanted = Some((0, region(0, 300)));
        track.accept(0, region(0, 300), Err("The file is broken.".to_owned()));
        assert_eq!(track.error.as_deref(), Some("The file is broken."));
    }

    #[test]
    fn fits_the_mode_to_the_rows_unless_chosen() {
        let mut track = track();
        track.wanted = Some((0, region(0, 300)));
        track.accept(0, region(0, 300), Ok(loaded(region(0, 300), 20)));
        assert_eq!(track.mode(0), Mode::Squished);
        track.mode = Some(Mode::Collapsed);
        assert_eq!(track.mode(0), Mode::Collapsed);
    }

    #[test]
    fn rejects_files_that_are_not_tracks() {
        let error = open(PathBuf::from("/data/calls.vcf.gz")).unwrap_err();
        assert_eq!(error, "The file calls.vcf.gz is not a BED or GFF3 file.");
    }

    #[test]
    fn asks_for_bgzip_when_the_file_is_plain() {
        let error = open(PathBuf::from("/data/genes.bed")).unwrap_err();
        assert_eq!(
            error,
            "Compress genes.bed with bgzip, then index it with tabix."
        );
    }

    #[test]
    fn asks_for_an_index_when_it_is_missing() {
        let scratch = Scratch::new("track-index");
        let error = open(scratch.file("genes.bed.gz", b"")).unwrap_err();
        assert_eq!(
            error,
            "There is no index genes.bed.gz.tbi. Index the file with tabix."
        );
    }

    #[test]
    fn reports_broken_indexes() {
        let scratch = Scratch::new("track-broken");
        let _ = scratch.file("genes.gff3.gz.tbi", b"TBI");
        let error = open(scratch.file("genes.gff3.gz", b"")).unwrap_err();
        assert_eq!(
            error,
            "The index genes.gff3.gz.tbi is not valid. The file is not compressed with bgzip."
        );
    }
}
