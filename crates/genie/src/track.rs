use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use genie_core::alignment::{Alignment, DUPLICATE, Filter};
use genie_core::pileup::{Coverage, Read, downsample};
use genie_core::{Assembly, Feature, Named, Names, Region};
use genie_formats::bgzf::Bgzf;
use genie_formats::features::{self, Kind};
use genie_formats::{bai, bam, tabix};
use genie_render::features::Mode;
use genie_render::scene::Hue;
use genie_render::{alignments, rows};

use crate::paths::file_name;

const FEATURE_SPAN: u64 = 10_000_000;
const READ_SPAN: u64 = 30_000;
const WINDOW: u64 = 50;
const DEPTH: usize = 100;
const READ_GAP: u64 = 2;
const MARGIN: u64 = 1_000;

#[derive(Debug)]
pub enum Format {
    Features {
        kind: Kind,
        index: tabix::Index,
    },
    Reads {
        index: bai::Index,
        contigs: Vec<String>,
    },
}

#[derive(Debug)]
pub struct Source {
    pub path: PathBuf,
    pub name: String,
    pub format: Format,
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Source {
    pub fn contigs(&self) -> &[String] {
        match &self.format {
            Format::Features { index, .. } => index.names(),
            Format::Reads { contigs, .. } => contigs,
        }
    }

    pub fn has_reads(&self) -> bool {
        matches!(self.format, Format::Reads { .. })
    }

    pub fn span(&self) -> u64 {
        if self.has_reads() {
            READ_SPAN
        } else {
            FEATURE_SPAN
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub duplicates: bool,
    pub clips: bool,
    pub downsample: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            duplicates: false,
            clips: false,
            downsample: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Duplicates,
    Clips,
    Downsample,
}

impl Options {
    #[must_use]
    pub fn toggled(self, toggle: Toggle) -> Self {
        let mut options = self;
        let flag = match toggle {
            Toggle::Duplicates => &mut options.duplicates,
            Toggle::Clips => &mut options.clips,
            Toggle::Downsample => &mut options.downsample,
        };
        *flag = !*flag;
        options
    }

    pub fn is_on(self, toggle: Toggle) -> bool {
        match toggle {
            Toggle::Duplicates => self.duplicates,
            Toggle::Clips => self.clips,
            Toggle::Downsample => self.downsample,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Annotation {
    pub features: Vec<Feature>,
    pub rows: Vec<usize>,
    pub count: usize,
}

#[derive(Debug, PartialEq)]
pub struct Pile {
    pub coverage: Coverage,
    pub reference: Vec<u8>,
    pub offset: u64,
    pub reads: Vec<Read>,
    pub rows: Vec<usize>,
    pub count: usize,
}

impl Pile {
    pub fn packed(&self, visible: Region) -> (Vec<usize>, usize) {
        let shown: Vec<usize> = (0..self.reads.len())
            .filter(|&index| {
                let span = self.reads[index].extent;
                span.start() <= visible.end() && span.end() >= visible.start()
            })
            .collect();
        let packed = rows::pack(&shown, |&index| self.reads[index].extent, READ_GAP);
        let mut rows = vec![0; self.reads.len()];
        for (&index, &row) in shown.iter().zip(&packed) {
            rows[index] = row;
        }
        (rows, count(&packed))
    }

    pub fn base(&self, position: u64) -> Option<u8> {
        let index = usize::try_from(position.checked_sub(self.offset)?).ok()?;
        self.reference.get(index).copied()
    }
}

#[derive(Debug, PartialEq)]
pub enum Content {
    Features(Annotation),
    Reads(Pile),
}

#[derive(Debug, PartialEq)]
pub struct Loaded {
    pub region: Region,
    pub options: Options,
    pub content: Content,
}

impl Loaded {
    pub fn features(region: Region, features: Vec<Feature>) -> Self {
        let rows = rows::pack(&features, Feature::span, 0);
        let count = count(&rows);
        Self {
            region,
            options: Options::default(),
            content: Content::Features(Annotation {
                features,
                rows,
                count,
            }),
        }
    }

    pub fn reads(
        region: Region,
        alignments: Vec<Alignment>,
        reference: Vec<u8>,
        offset: u64,
        options: Options,
    ) -> Self {
        let mut coverage = Coverage::new(region);
        for alignment in &alignments {
            coverage.add(alignment);
        }
        let alignments = if options.downsample {
            downsample(alignments, WINDOW, DEPTH)
        } else {
            alignments
        };
        let reads: Vec<Read> = alignments
            .into_iter()
            .map(|alignment| Read::new(alignment, &reference, offset, options.clips))
            .collect();
        let rows = rows::pack(&reads, |read| read.extent, READ_GAP);
        let count = count(&rows);
        Self {
            region,
            options,
            content: Content::Reads(Pile {
                coverage,
                reference,
                offset,
                reads,
                rows,
                count,
            }),
        }
    }

    pub fn count(&self) -> usize {
        match &self.content {
            Content::Features(annotation) => annotation.count,
            Content::Reads(pile) => pile.count,
        }
    }
}

fn count(rows: &[usize]) -> usize {
    rows.iter().max().map_or(0, |row| row + 1)
}

#[derive(Debug)]
pub struct Track {
    pub id: u64,
    pub source: Arc<Source>,
    pub contigs: Vec<Option<String>>,
    pub names: Option<Arc<Names>>,
    pub hue: Hue,
    pub mode: Option<Mode>,
    pub options: Options,
    pub loaded: Option<(usize, Arc<Loaded>)>,
    pub wanted: Option<(usize, Region)>,
    pub error: Option<String>,
}

impl Track {
    pub fn new(id: u64, source: Arc<Source>, assembly: &Assembly) -> Self {
        let contigs = contig_names(source.contigs(), assembly);
        Self {
            id,
            source,
            contigs,
            names: None,
            hue: Hue::default(),
            mode: None,
            options: Options::default(),
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
        self.mode.unwrap_or_else(|| {
            let count = self.data(contig).map_or(0, Loaded::count);
            if self.source.has_reads() {
                alignments::fit(count)
            } else {
                Mode::fit(count)
            }
        })
    }

    pub fn needs(&self, contig: usize, visible: Region) -> bool {
        let covers = |(wanted, region): (usize, Region)| {
            wanted == contig && region.start() <= visible.start() && visible.end() <= region.end()
        };
        let loaded = self
            .data(contig)
            .filter(|data| data.options == self.options)
            .map(|data| (contig, data.region));
        self.contigs.get(contig).is_some_and(Option::is_some)
            && visible.len() <= self.source.span()
            && !loaded.is_some_and(covers)
            && !self.wanted.is_some_and(covers)
    }

    pub fn accept(&mut self, contig: usize, region: Region, result: Result<Arc<Loaded>, String>) {
        let stale = result
            .as_ref()
            .is_ok_and(|data| data.options != self.options);
        if self.wanted != Some((contig, region)) || stale {
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
    Toggle(Toggle),
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
        Change::Toggle(toggle) => {
            let track = &mut tracks[position];
            track.options = track.options.toggled(toggle);
            track.wanted = None;
        }
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
    is_bam(path) || Kind::of(&file_name(path)).is_some()
}

fn is_bam(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bam"))
}

pub fn open(path: PathBuf) -> Result<Source, String> {
    let name = file_name(&path);
    if is_bam(&path) {
        return open_reads(path, name);
    }
    let kind = Kind::of(&name)
        .ok_or_else(|| format!("The file {name} is not a BED, GFF3 or BAM file."))?;
    let compressed = path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("gz") || extension.eq_ignore_ascii_case("bgz")
    });
    if !compressed {
        return Err(format!(
            "Compress {name} with bgzip, then index it with tabix."
        ));
    }
    let file = File::open(with_suffix(&path, ".tbi"))
        .map_err(|_| format!("There is no index {name}.tbi. Index the file with tabix."))?;
    let index = tabix::Index::read(file)
        .map_err(|error| format!("The index {name}.tbi is not valid. {error}"))?;
    Ok(Source {
        path,
        name,
        format: Format::Features { kind, index },
    })
}

fn open_reads(path: PathBuf, name: String) -> Result<Source, String> {
    let file = [with_suffix(&path, ".bai"), path.with_extension("bai")]
        .into_iter()
        .find_map(|candidate| File::open(candidate).ok())
        .ok_or_else(|| {
            format!("There is no index {name}.bai. Index the file with samtools index.")
        })?;
    let index = bai::Index::read(file)
        .map_err(|error| format!("The index {name}.bai is not valid. {error}"))?;
    let file = File::open(&path).map_err(|error| error.to_string())?;
    let header = bam::header(&mut Bgzf::new(file)).map_err(|error| error.to_string())?;
    let contigs = header
        .references
        .into_iter()
        .map(|(contig, _)| contig)
        .collect();
    Ok(Source {
        path,
        name,
        format: Format::Reads { index, contigs },
    })
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut text = path.as_os_str().to_owned();
    text.push(suffix);
    PathBuf::from(text)
}

pub fn load(
    source: &Source,
    contig: &str,
    region: Region,
    options: Options,
    reference: impl FnOnce(Region) -> Result<Vec<u8>, String>,
) -> Result<Loaded, String> {
    let file = File::open(&source.path).map_err(|error| error.to_string())?;
    let mut reader = Bgzf::new(file);
    match &source.format {
        Format::Features { kind, index } => {
            let (region, features) = features::query(*kind, index, &mut reader, contig, region)
                .map_err(|error| error.to_string())?;
            Ok(Loaded::features(region, features))
        }
        Format::Reads { index, contigs } => {
            let id = contigs
                .iter()
                .position(|known| known == contig)
                .ok_or_else(|| format!("The file has no contig {contig}."))?;
            let filter = if options.duplicates {
                Filter::default().showing(DUPLICATE)
            } else {
                Filter::default()
            };
            let mut alignments = Vec::new();
            bam::query(index, &mut reader, id, region, |alignment| {
                if filter.keeps(&alignment) {
                    alignments.push(alignment);
                }
                Ok(())
            })
            .map_err(|error| error.to_string())?;
            let window = Region::between(
                region.start().saturating_sub(MARGIN),
                region.end().saturating_add(MARGIN),
            );
            let bases = reference(window)?;
            Ok(Loaded::reads(
                region,
                alignments,
                bases,
                window.start(),
                options,
            ))
        }
    }
}

pub fn names(source: &Source, assembly: &Assembly) -> Result<Names, String> {
    let Format::Features { kind, .. } = &source.format else {
        return Ok(Names::default());
    };
    let file = File::open(&source.path).map_err(|error| error.to_string())?;
    let entries = features::names(*kind, Bgzf::new(file)).map_err(|error| error.to_string())?;
    Ok(Names::new(entries.into_iter().filter_map(|entry| {
        Some(Named {
            contig: assembly.find(&entry.contig)?,
            name: entry.name,
            span: entry.span,
        })
    })))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::alignment::{Alignment, DUPLICATE, Op, QC_FAIL, SECONDARY};
    use genie_core::{Assembly, Contig, Feature, Region, Strand};
    use genie_formats::features::Kind;
    use genie_formats::tabix;
    use genie_render::features::Mode;
    use genie_render::scene::Hue;

    use super::{
        Change, Content, Format, Loaded, Options, Source, Toggle, Track, change, contig_names,
        load, names, open,
    };
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
            format: Format::Features {
                kind: Kind::Bed,
                index: tabix::Index::default(),
            },
        };
        let mut track = Track::new(7, Arc::new(source), &assembly());
        track.contigs = vec![Some("1".to_owned()), None, None];
        track
    }

    fn loaded(region: Region, count: usize) -> Arc<Loaded> {
        let features = (0..count)
            .map(|_| Feature::new("", region, Strand::Forward))
            .collect();
        Arc::new(Loaded::features(region, features))
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

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../genie-formats/tests/data")
            .join(name)
    }

    fn no_reference(_: Region) -> Result<Vec<u8>, String> {
        Err("There is no reference.".to_owned())
    }

    #[test]
    fn opens_loads_and_names_tracks_made_by_htslib() {
        let source = open(fixture("genes.gff3.gz")).unwrap();
        assert!(matches!(
            source.format,
            Format::Features {
                kind: Kind::Gff,
                ..
            }
        ));
        let window = region(50_000, 51_000);
        let loaded = load(&source, "chr1", window, Options::default(), no_reference).unwrap();
        assert_eq!(loaded.region, region(50_000, 57_400));
        let Content::Features(annotation) = &loaded.content else {
            panic!("expected features");
        };
        assert!(
            annotation
                .features
                .iter()
                .any(|feature| feature.name() == "GENE1003")
        );
        let names = names(&source, &assembly()).unwrap();
        let gene = names.find("gene1003").unwrap();
        assert_eq!((gene.contig, gene.span), (0, region(50_366, 57_400)));
    }

    #[test]
    fn opens_and_loads_reads_made_by_samtools() {
        let source = open(fixture("reads.bam")).unwrap();
        assert_eq!(source.contigs(), ["chr1", "chr2"]);
        let reference = |window: Region| Ok(vec![b'A'; window.len() as usize]);
        let window = region(5_000, 8_000);
        let loaded = load(&source, "chr1", window, Options::default(), reference).unwrap();
        let Content::Reads(pile) = &loaded.content else {
            panic!("expected reads");
        };
        assert!(pile.reads.len() >= 10, "{}", pile.reads.len());
        assert!(pile.coverage.depth(6_000) > 0);
        assert_eq!(pile.base(6_000), Some(b'A'));
        assert!(
            pile.reads
                .iter()
                .all(|read| !read.alignment.has(DUPLICATE | QC_FAIL | SECONDARY))
        );
    }

    fn alignment(start: u64, cigar: Vec<(Op, u32)>) -> Alignment {
        let length = cigar
            .iter()
            .filter(|(op, _)| op.consumes_read())
            .map(|(_, length)| *length as usize)
            .sum();
        Alignment {
            name: format!("read{start}"),
            start,
            flags: 0,
            mapq: 60,
            cigar,
            bases: vec![b'A'; length],
            qualities: Vec::new(),
        }
    }

    #[test]
    fn toggles_flip_options_and_drop_the_request() {
        let mut tracks = three();
        tracks[1].wanted = Some((0, region(0, 300)));
        change(&mut tracks, 1, Change::Toggle(Toggle::Duplicates));
        assert!(tracks[1].options.duplicates);
        assert_eq!(tracks[1].wanted, None);
        change(&mut tracks, 1, Change::Toggle(Toggle::Duplicates));
        assert!(!tracks[1].options.duplicates);
    }

    #[test]
    fn data_for_other_options_is_reloaded_or_dropped() {
        let mut track = track();
        track.wanted = Some((0, region(0, 300)));
        track.accept(0, region(0, 300), Ok(loaded(region(0, 300), 1)));
        assert!(!track.needs(0, region(100, 200)));
        track.options = track.options.toggled(Toggle::Clips);
        assert!(track.needs(0, region(100, 200)));
        track.wanted = Some((0, region(0, 300)));
        track.accept(0, region(0, 300), Ok(loaded(region(0, 300), 2)));
        assert_eq!(track.data(0).unwrap().count(), 1);
        assert_eq!(track.wanted, Some((0, region(0, 300))));
    }

    #[test]
    fn read_options_shape_the_pile() {
        let deep: Vec<Alignment> = (0..150)
            .map(|index| alignment(10 + index % 10, vec![(Op::SoftClip, 2), (Op::Match, 10)]))
            .collect();
        let pile = |options| {
            let loaded = Loaded::reads(region(0, 100), deep.clone(), vec![b'A'; 100], 0, options);
            let Content::Reads(pile) = loaded.content else {
                panic!("expected reads");
            };
            pile
        };
        let fixed = pile(Options::default());
        assert_eq!(fixed.reads.len(), 100);
        assert_eq!(fixed.reads[0].parts.clips.len(), 0);
        let open = pile(Options {
            clips: true,
            downsample: false,
            ..Options::default()
        });
        assert_eq!(open.reads.len(), 150);
        assert_eq!(open.reads[0].parts.clips.len(), 2);
    }

    #[test]
    fn duplicates_load_when_shown() {
        let source = open(fixture("reads.bam")).unwrap();
        let reference = |window: Region| Ok(vec![b'A'; window.len() as usize]);
        let options = Options {
            duplicates: true,
            ..Options::default()
        };
        let loaded = load(&source, "chr1", region(1_000, 41_000), options, reference).unwrap();
        let Content::Reads(pile) = &loaded.content else {
            panic!("expected reads");
        };
        assert!(pile.reads.iter().any(|read| read.alignment.has(DUPLICATE)));
    }

    #[test]
    fn packing_for_a_view_skips_reads_outside_it() {
        let alignments = [0, 50, 500, 510, 520]
            .map(|start| alignment(start, vec![(Op::Match, 100)]))
            .to_vec();
        let options = Options::default();
        let loaded = Loaded::reads(region(0, 1_000), alignments, vec![b'A'; 1_000], 0, options);
        let Content::Reads(pile) = &loaded.content else {
            panic!("expected reads");
        };
        assert_eq!(pile.count, 3);
        let (rows, count) = pile.packed(region(0, 200));
        assert_eq!(count, 2);
        let mut shown: Vec<usize> = pile
            .reads
            .iter()
            .zip(&rows)
            .filter(|(read, _)| read.alignment.start < 200)
            .map(|(_, row)| *row)
            .collect();
        shown.sort_unstable();
        assert_eq!(shown, [0, 1]);
    }

    #[test]
    fn asks_for_a_bam_index_when_it_is_missing() {
        let scratch = Scratch::new("track-bam");
        let error = open(scratch.file("reads.bam", b"")).unwrap_err();
        assert_eq!(
            error,
            "There is no index reads.bam.bai. Index the file with samtools index."
        );
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
        assert!(!track.needs(0, region(0, 20_000_000)));
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
        assert_eq!(
            error,
            "The file calls.vcf.gz is not a BED, GFF3 or BAM file."
        );
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
