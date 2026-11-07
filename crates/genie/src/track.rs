use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use genie_core::alignment::{Alignment, DUPLICATE, Filter, REVERSE, expected_inserts};
use genie_core::pileup::{Coverage, Read, downsample, sort_rows};
use genie_core::signal::{self, Summary};
use genie_core::variant::Variant;
use genie_core::{Assembly, Feature, Named, Names, Region};
use genie_formats::bgzf::Bgzf;
use genie_formats::features::{self, Kind, Table};
use genie_formats::{bai, bam, bbi, bedgraph, tabix, vcf};
use genie_render::features::Mode;
use genie_render::scene::Hue;
use genie_render::signal::Style;
use genie_render::{alignments, rows, variants};

use crate::paths::file_name;

const FEATURE_SPAN: u64 = 10_000_000;
const VARIANT_SPAN: u64 = 1_000_000;
const READ_SPAN: u64 = 30_000;
const WINDOW: u64 = 50;
const DEPTH: usize = 100;
const READ_GAP: u64 = 2;
const MARGIN: u64 = 1_000;
const PLAIN_LIMIT: u64 = 50 << 20;

#[derive(Debug)]
pub enum Format {
    Features {
        kind: Kind,
        index: tabix::Index,
    },
    Table {
        kind: Kind,
        table: Table<Feature>,
    },
    Variants {
        index: tabix::Index,
        samples: Vec<String>,
    },
    VariantTable {
        table: Table<Variant>,
        samples: Vec<String>,
    },
    Reads {
        index: bai::Index,
        contigs: Vec<String>,
    },
    BigWig {
        index: bbi::Index,
    },
    BigBed {
        index: bbi::Index,
    },
    BedGraph {
        index: tabix::Index,
    },
    BedGraphTable {
        table: Table<Summary>,
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
            Format::Features { index, .. }
            | Format::Variants { index, .. }
            | Format::BedGraph { index } => index.names(),
            Format::Table { table, .. } => table.contigs(),
            Format::VariantTable { table, .. } => table.contigs(),
            Format::BedGraphTable { table } => table.contigs(),
            Format::Reads { contigs, .. } => contigs,
            Format::BigWig { index } | Format::BigBed { index } => index.names(),
        }
    }

    pub fn is_bigbed(&self) -> bool {
        matches!(self.format, Format::BigBed { .. })
    }

    pub fn has_colors(&self) -> bool {
        matches!(
            self.format,
            Format::Features {
                kind: Kind::Bed,
                ..
            } | Format::Table {
                kind: Kind::Bed,
                ..
            } | Format::BigBed { .. }
        )
    }

    pub fn has_signal(&self) -> bool {
        matches!(
            self.format,
            Format::BigWig { .. } | Format::BedGraph { .. } | Format::BedGraphTable { .. }
        )
    }

    pub fn has_reads(&self) -> bool {
        matches!(self.format, Format::Reads { .. })
    }

    pub fn has_variants(&self) -> bool {
        matches!(
            self.format,
            Format::Variants { .. } | Format::VariantTable { .. }
        )
    }

    pub fn samples(&self) -> &[String] {
        match &self.format {
            Format::Variants { samples, .. } | Format::VariantTable { samples, .. } => samples,
            _ => &[],
        }
    }

    pub fn span(&self) -> u64 {
        match self.format {
            Format::Reads { .. } => READ_SPAN,
            Format::Variants { .. } | Format::VariantTable { .. } => VARIANT_SPAN,
            Format::BigWig { .. } | Format::BigBed { .. } | Format::BedGraphTable { .. } => {
                u64::MAX
            }
            _ => FEATURE_SPAN,
        }
    }
}

pub fn detailed(visible: Region) -> bool {
    visible.len() <= FEATURE_SPAN
}

pub fn step(bases_per_pixel: f64) -> u64 {
    let desired = (bases_per_pixel / 2.0).floor().max(1.0) as u64;
    let mut step = 1;
    while step * 4 <= desired {
        step *= 4;
    }
    step
}

#[derive(Debug, PartialEq)]
pub struct Signal {
    pub summaries: Vec<Summary>,
    pub step: u64,
}

#[derive(Debug, PartialEq)]
pub struct Calls {
    pub variants: Vec<Variant>,
    pub samples: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Grouping {
    #[default]
    None,
    Phase,
    Strand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub duplicates: bool,
    pub clips: bool,
    pub downsample: bool,
    pub group: Grouping,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            duplicates: false,
            clips: false,
            downsample: true,
            group: Grouping::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plot {
    pub style: Style,
    pub linked: bool,
    pub pairs: bool,
    pub colors: bool,
}

impl Default for Plot {
    fn default() -> Self {
        Self {
            style: Style::default(),
            linked: false,
            pairs: true,
            colors: true,
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
    pub groups: Vec<(usize, String)>,
    pub grouping: Grouping,
}

fn group_key(read: &Read, grouping: Grouping) -> (u8, i64) {
    match grouping {
        Grouping::None => (0, 0),
        Grouping::Phase => read.alignment.tags.phase.map_or((1, 0), |phase| (0, phase)),
        Grouping::Strand => (u8::from(read.alignment.has(REVERSE)), 0),
    }
}

fn group_label(grouping: Grouping, key: (u8, i64)) -> String {
    match (grouping, key) {
        (Grouping::Phase, (0, phase)) => format!("Haplotype {phase}"),
        (Grouping::Phase, _) => "No haplotype".to_owned(),
        (Grouping::Strand, (0, _)) => "Forward strand".to_owned(),
        (Grouping::Strand, _) => "Reverse strand".to_owned(),
        (Grouping::None, _) => String::new(),
    }
}

fn pack(
    reads: &[Read],
    members: &[usize],
    grouping: Grouping,
) -> (Vec<usize>, Vec<(usize, String)>) {
    let extent = |&index: &usize| reads[index].extent;
    if grouping == Grouping::None {
        return (rows::pack(members, extent, READ_GAP), Vec::new());
    }
    let keys: Vec<(u8, i64)> = members
        .iter()
        .map(|&index| group_key(&reads[index], grouping))
        .collect();
    let mut order = keys.clone();
    order.sort_unstable();
    order.dedup();
    let mut rows = vec![0; members.len()];
    let mut groups = Vec::new();
    let mut next = 0;
    for key in order {
        let places: Vec<usize> = (0..members.len())
            .filter(|&place| keys[place] == key)
            .collect();
        let chosen: Vec<usize> = places.iter().map(|&place| members[place]).collect();
        let layout = rows::pack(&chosen, extent, READ_GAP);
        for (&place, &row) in places.iter().zip(&layout) {
            rows[place] = next + row;
        }
        groups.push((next, group_label(grouping, key)));
        next += count(&layout);
    }
    (rows, groups)
}

impl Pile {
    pub fn packed(&self, visible: Region) -> (Vec<usize>, usize, Vec<(usize, String)>) {
        let shown: Vec<usize> = (0..self.reads.len())
            .filter(|&index| {
                let span = self.reads[index].extent;
                span.start() <= visible.end() && span.end() >= visible.start()
            })
            .collect();
        let (packed, groups) = pack(&self.reads, &shown, self.grouping);
        let mut rows = vec![0; self.reads.len()];
        for (&index, &row) in shown.iter().zip(&packed) {
            rows[index] = row;
        }
        (rows, count(&packed), groups)
    }

    pub fn starts(&self) -> Vec<usize> {
        self.groups.iter().map(|(first, _)| *first).collect()
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
    Variants(Calls),
    Signal(Signal),
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
        contig: usize,
        alignments: Vec<Alignment>,
        reference: Vec<u8>,
        offset: u64,
        options: Options,
    ) -> Self {
        let mut coverage = Coverage::new(region);
        for alignment in &alignments {
            coverage.add(alignment);
        }
        let range = expected_inserts(alignments.iter(), contig);
        let alignments = if options.downsample {
            downsample(alignments, WINDOW, DEPTH)
        } else {
            alignments
        };
        let reads: Vec<Read> = alignments
            .into_iter()
            .map(|alignment| {
                let mut read = Read::new(alignment, &reference, offset, options.clips);
                read.pairing = read.alignment.pairing(contig, range);
                read
            })
            .collect();
        let members: Vec<usize> = (0..reads.len()).collect();
        let (rows, groups) = pack(&reads, &members, options.group);
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
                groups,
                grouping: options.group,
            }),
        }
    }

    pub fn variants(region: Region, variants: Vec<Variant>, samples: usize) -> Self {
        Self {
            region,
            options: Options::default(),
            content: Content::Variants(Calls { variants, samples }),
        }
    }

    pub fn signal(region: Region, summaries: Vec<Summary>, step: u64) -> Self {
        Self {
            region,
            options: Options::default(),
            content: Content::Signal(Signal { summaries, step }),
        }
    }

    pub fn count(&self) -> usize {
        match &self.content {
            Content::Features(annotation) => annotation.count,
            Content::Reads(pile) => pile.count,
            Content::Variants(calls) => calls.samples,
            Content::Signal(_) => 0,
        }
    }

    fn step(&self) -> u64 {
        match &self.content {
            Content::Signal(signal) => signal.step,
            _ => 1,
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
    pub plot: Plot,
    pub height: Option<f32>,
    pub loaded: Option<(usize, Arc<Loaded>)>,
    pub wanted: Option<(usize, Region)>,
    pub error: Option<String>,
    pub order: Option<(u64, Vec<usize>)>,
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
            plot: Plot::default(),
            height: None,
            loaded: None,
            wanted: None,
            error: None,
            order: None,
        }
    }

    pub fn rows<'a>(&'a self, pile: &'a Pile) -> &'a [usize] {
        self.order.as_ref().map_or(&pile.rows, |(_, rows)| rows)
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
            } else if self.source.has_variants() {
                variants::fit(count)
            } else {
                Mode::fit(count)
            }
        })
    }

    pub fn needs(&self, contig: usize, visible: Region, step: u64) -> bool {
        let covers = |(wanted, region): (usize, Region)| {
            wanted == contig && region.start() <= visible.start() && visible.end() <= region.end()
        };
        let fits = |data: &Loaded| {
            !self.source.is_bigbed()
                || matches!(data.content, Content::Features(_)) == detailed(visible)
        };
        let loaded = self
            .data(contig)
            .filter(|data| data.options == self.options && data.step() <= step && fits(data))
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
                self.order = None;
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
    Style(Style),
    Link,
    Pairs,
    Colors,
    Sort(u64),
    Group(Grouping),
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
        Change::Style(style) => tracks[position].plot.style = style,
        Change::Link => {
            let plot = &mut tracks[position].plot;
            plot.linked = !plot.linked;
        }
        Change::Pairs => {
            let plot = &mut tracks[position].plot;
            plot.pairs = !plot.pairs;
        }
        Change::Colors => {
            let plot = &mut tracks[position].plot;
            plot.colors = !plot.colors;
        }
        Change::Group(grouping) => {
            let track = &mut tracks[position];
            track.options.group = if track.options.group == grouping {
                Grouping::None
            } else {
                grouping
            };
            track.wanted = None;
        }
        Change::Sort(at) => {
            let track = &mut tracks[position];
            track.order = track
                .loaded
                .as_ref()
                .and_then(|(_, data)| match &data.content {
                    Content::Reads(pile) => {
                        Some((at, sort_rows(&pile.reads, &pile.rows, &pile.starts(), at)))
                    }
                    _ => None,
                });
        }
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
    let name = file_name(path);
    is_bam(path)
        || is_vcf(&name)
        || is_signal(&name)
        || is_bigbed(&name)
        || Kind::of(&name).is_some()
}

fn is_bam(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bam"))
}

fn inner_extension(name: &str, wanted: &[&str]) -> bool {
    let path = Path::new(name);
    let stem = if is_compressed(path) {
        path.file_stem().map(Path::new)
    } else {
        Some(path)
    };
    stem.and_then(Path::extension).is_some_and(|extension| {
        wanted
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    })
}

fn is_vcf(name: &str) -> bool {
    inner_extension(name, &["vcf"])
}

fn is_bigwig(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("bw") || extension.eq_ignore_ascii_case("bigwig")
    })
}

fn is_bigbed(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("bb") || extension.eq_ignore_ascii_case("bigbed")
    })
}

fn is_bedgraph(name: &str) -> bool {
    inner_extension(name, &["bedgraph", "bdg"])
}

fn is_signal(name: &str) -> bool {
    is_bigwig(name) || is_bedgraph(name)
}

fn is_compressed(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("gz") || extension.eq_ignore_ascii_case("bgz")
    })
}

fn plain(path: &Path, name: &str) -> Result<BufReader<File>, String> {
    let size = fs::metadata(path).map_err(|error| error.to_string())?.len();
    fits(name, size)?;
    File::open(path)
        .map(BufReader::new)
        .map_err(|error| error.to_string())
}

fn tabix_index(path: &Path, name: &str) -> Result<tabix::Index, String> {
    let (file, suffix) = [".tbi", ".csi"]
        .into_iter()
        .find_map(|suffix| Some((File::open(with_suffix(path, suffix)).ok()?, suffix)))
        .ok_or_else(|| format!("There is no index {name}.tbi. Index the file with tabix."))?;
    tabix::Index::read(file)
        .map_err(|error| format!("The index {name}{suffix} is not valid. {error}"))
}

pub fn open(path: PathBuf) -> Result<Source, String> {
    let name = file_name(&path);
    if is_bam(&path) {
        return open_reads(path, name);
    }
    if is_vcf(&name) {
        return open_variants(path, name);
    }
    if is_bigbed(&name) || is_bigwig(&name) {
        return open_bbi(path, name);
    }
    if is_signal(&name) {
        return open_signal(path, name);
    }
    let kind = Kind::of(&name).ok_or_else(|| {
        format!(
            "The file {name} is not a BED, GFF3, GTF, VCF, BAM, bigWig, bigBed or bedGraph file."
        )
    })?;
    if !is_compressed(&path) {
        let table = features::read_table(kind, plain(&path, &name)?)
            .map_err(|error| format!("The file {name} is not valid. {error}"))?;
        return Ok(Source {
            path,
            name,
            format: Format::Table { kind, table },
        });
    }
    let index = tabix_index(&path, &name)?;
    Ok(Source {
        path,
        name,
        format: Format::Features { kind, index },
    })
}

fn open_variants(path: PathBuf, name: String) -> Result<Source, String> {
    if !is_compressed(&path) {
        let (header, table) = vcf::read_table(plain(&path, &name)?)
            .map_err(|error| format!("The file {name} is not valid. {error}"))?;
        return Ok(Source {
            path,
            name,
            format: Format::VariantTable {
                table,
                samples: header.samples,
            },
        });
    }
    let index = tabix_index(&path, &name)?;
    let file = File::open(&path).map_err(|error| error.to_string())?;
    let header = vcf::header(Bgzf::new(file)).map_err(|error| error.to_string())?;
    Ok(Source {
        path,
        name,
        format: Format::Variants {
            index,
            samples: header.samples,
        },
    })
}

fn open_bbi(path: PathBuf, name: String) -> Result<Source, String> {
    let mut file = File::open(&path).map_err(|error| error.to_string())?;
    let index = bbi::Index::read(&mut file)
        .map_err(|error| format!("The file {name} is not valid. {error}"))?;
    let format = if index.is_bed() {
        Format::BigBed { index }
    } else {
        Format::BigWig { index }
    };
    Ok(Source { path, name, format })
}

fn open_signal(path: PathBuf, name: String) -> Result<Source, String> {
    let format = if is_compressed(&path) {
        Format::BedGraph {
            index: tabix_index(&path, &name)?,
        }
    } else {
        let table = bedgraph::read_table(plain(&path, &name)?)
            .map_err(|error| format!("The file {name} is not valid. {error}"))?;
        Format::BedGraphTable { table }
    };
    Ok(Source { path, name, format })
}

fn fits(name: &str, size: u64) -> Result<(), String> {
    if size > PLAIN_LIMIT {
        return Err(format!(
            "The file {name} is larger than 50 MB. Compress it with bgzip, then index it with tabix."
        ));
    }
    Ok(())
}

fn open_reads(path: PathBuf, name: String) -> Result<Source, String> {
    let file = [
        with_suffix(&path, ".bai"),
        path.with_extension("bai"),
        with_suffix(&path, ".csi"),
    ]
    .into_iter()
    .find_map(|candidate| File::open(candidate).ok())
    .ok_or_else(|| format!("There is no index {name}.bai. Index the file with samtools index."))?;
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
    step: u64,
    detail: bool,
    reference: impl FnOnce(Region) -> Result<Vec<u8>, String>,
) -> Result<Loaded, String> {
    let reader = || {
        File::open(&source.path)
            .map(Bgzf::new)
            .map_err(|error| error.to_string())
    };
    match &source.format {
        Format::BigBed { index } => {
            let file = File::open(&source.path).map_err(|error| error.to_string())?;
            let mut file = BufReader::new(file);
            if detail {
                let features = index
                    .features(&mut file, contig, region)
                    .map_err(|error| error.to_string())?;
                return Ok(Loaded::features(region, features));
            }
            let summaries = index
                .summaries(&mut file, contig, region, step)
                .map_err(|error| error.to_string())?;
            Ok(Loaded::signal(region, summaries, step))
        }
        Format::BigWig { index } => {
            let file = File::open(&source.path).map_err(|error| error.to_string())?;
            let summaries = index
                .summaries(&mut BufReader::new(file), contig, region, step)
                .map_err(|error| error.to_string())?;
            Ok(Loaded::signal(region, summaries, step))
        }
        Format::BedGraphTable { table } => {
            let records = table.query(contig, region);
            Ok(Loaded::signal(region, signal::reduce(&records, step), step))
        }
        Format::BedGraph { index } => {
            let records = bedgraph::query(index, &mut reader()?, contig, region)
                .map_err(|error| error.to_string())?;
            Ok(Loaded::signal(region, signal::reduce(&records, step), step))
        }
        Format::Table { table, .. } => Ok(Loaded::features(region, table.query(contig, region))),
        Format::VariantTable { table, samples } => Ok(Loaded::variants(
            region,
            table.query(contig, region),
            samples.len(),
        )),
        Format::Variants { index, samples } => {
            let variants = vcf::query(index, &mut reader()?, contig, region)
                .map_err(|error| error.to_string())?;
            Ok(Loaded::variants(region, variants, samples.len()))
        }
        Format::Features { kind, index } => {
            let (region, features) = features::query(*kind, index, &mut reader()?, contig, region)
                .map_err(|error| error.to_string())?;
            Ok(Loaded::features(region, features))
        }
        Format::Reads { index, contigs } => {
            let mut reader = reader()?;
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
                id,
                alignments,
                bases,
                window.start(),
                options,
            ))
        }
    }
}

fn bigbed_names(
    index: &bbi::Index,
    reader: &mut BufReader<File>,
) -> Result<Vec<features::Entry>, genie_formats::Error> {
    let whole = Region::between(0, u64::from(u32::MAX));
    let mut entries = Vec::new();
    for contig in index.names() {
        for feature in index.features(reader, contig, whole)? {
            if !feature.name().is_empty() {
                entries.push(features::Entry {
                    name: feature.name().to_owned(),
                    contig: contig.clone(),
                    span: feature.span(),
                });
            }
        }
    }
    Ok(entries)
}

pub fn names(source: &Source, assembly: &Assembly) -> Result<Names, String> {
    let file = || File::open(&source.path).map_err(|error| error.to_string());
    let entries = match &source.format {
        Format::Features { kind, .. } => features::names(*kind, Bgzf::new(file()?)),
        Format::Table { kind, .. } => features::names(*kind, BufReader::new(file()?)),
        Format::VariantTable { .. } => vcf::names(BufReader::new(file()?)),
        Format::Variants { .. } => {
            let size = fs::metadata(&source.path).map_or(u64::MAX, |metadata| metadata.len());
            if size > PLAIN_LIMIT {
                return Ok(Names::default());
            }
            vcf::names(Bgzf::new(file()?))
        }
        Format::BigBed { index } => {
            let size = fs::metadata(&source.path).map_or(u64::MAX, |metadata| metadata.len());
            if index.has_lookup() || size > PLAIN_LIMIT {
                return Ok(Names::default());
            }
            bigbed_names(index, &mut BufReader::new(file()?))
        }
        Format::Reads { .. }
        | Format::BigWig { .. }
        | Format::BedGraph { .. }
        | Format::BedGraphTable { .. } => return Ok(Names::default()),
    }
    .map_err(|error| error.to_string())?;
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

    use genie_core::alignment::{
        Alignment, DUPLICATE, MATE_REVERSE, Mate, Op, PAIRED, PROPER, Pairing, QC_FAIL, REVERSE,
        SECONDARY, Tags,
    };
    use genie_core::{Assembly, Contig, Feature, Names, Region, Strand};
    use genie_formats::features::Kind;
    use genie_formats::tabix;
    use genie_render::features::Mode;
    use genie_render::scene::Hue;
    use genie_render::signal::Style;

    use super::{
        Change, Content, Format, Grouping, Loaded, Options, Plot, Source, Toggle, Track, change,
        contig_names, fits, is_track, load, names, open, step,
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
    fn changes_set_the_signal_style_and_the_link() {
        let mut tracks = three();
        change(&mut tracks, 1, Change::Style(Style::Line));
        change(&mut tracks, 1, Change::Link);
        assert_eq!(
            tracks[1].plot,
            Plot {
                style: Style::Line,
                linked: true,
                ..Plot::default()
            }
        );
        change(&mut tracks, 1, Change::Link);
        assert!(!tracks[1].plot.linked);
    }

    #[test]
    fn changes_toggle_the_file_colors() {
        let mut tracks = three();
        assert!(tracks[1].plot.colors);
        assert!(tracks[1].source.has_colors());
        change(&mut tracks, 1, Change::Colors);
        assert!(!tracks[1].plot.colors);
    }

    #[test]
    fn changes_toggle_the_pair_colors() {
        let mut tracks = three();
        assert!(tracks[1].plot.pairs);
        tracks[1].wanted = Some((0, region(0, 10)));
        change(&mut tracks, 1, Change::Pairs);
        assert!(!tracks[1].plot.pairs);
        assert_eq!(tracks[1].wanted, Some((0, region(0, 10))));
    }

    #[test]
    fn grouping_packs_each_group_under_its_label() {
        let phased = |start: u64, phase: Option<i64>| Alignment {
            tags: Tags { phase, group: None },
            ..alignment(start, vec![(Op::Match, 10)])
        };
        let reads = vec![
            phased(10, Some(2)),
            phased(10, None),
            phased(12, Some(1)),
            phased(14, Some(2)),
            phased(40, Some(1)),
        ];
        let options = Options {
            group: Grouping::Phase,
            downsample: false,
            ..Options::default()
        };
        let loaded = Loaded::reads(region(0, 100), 0, reads, vec![b'A'; 100], 0, options);
        let Content::Reads(pile) = &loaded.content else {
            panic!("expected reads");
        };
        let labels: Vec<(usize, &str)> = pile
            .groups
            .iter()
            .map(|(row, label)| (*row, label.as_str()))
            .collect();
        assert_eq!(
            labels,
            [(0, "Haplotype 1"), (1, "Haplotype 2"), (3, "No haplotype")]
        );
        assert_eq!(pile.rows, [1, 3, 0, 2, 0]);
        assert_eq!(pile.count, 4);
        let (rows, count, groups) = pile.packed(region(30, 100));
        assert_eq!((rows[4], count), (0, 1));
        assert_eq!(groups, [(0, "Haplotype 1".to_owned())]);
    }

    #[test]
    fn changes_switch_the_grouping_and_reload() {
        let mut tracks = three();
        tracks[1].wanted = Some((0, region(0, 10)));
        change(&mut tracks, 1, Change::Group(Grouping::Phase));
        assert_eq!(tracks[1].options.group, Grouping::Phase);
        assert_eq!(tracks[1].wanted, None);
        change(&mut tracks, 1, Change::Group(Grouping::Strand));
        assert_eq!(tracks[1].options.group, Grouping::Strand);
        change(&mut tracks, 1, Change::Group(Grouping::Strand));
        assert_eq!(tracks[1].options.group, Grouping::None);
    }

    #[test]
    fn strand_groups_put_forward_reads_first() {
        let reads = vec![
            Alignment {
                flags: REVERSE,
                ..alignment(10, vec![(Op::Match, 10)])
            },
            alignment(10, vec![(Op::Match, 10)]),
        ];
        let options = Options {
            group: Grouping::Strand,
            ..Options::default()
        };
        let loaded = Loaded::reads(region(0, 100), 0, reads, vec![b'A'; 100], 0, options);
        let Content::Reads(pile) = &loaded.content else {
            panic!("expected reads");
        };
        let labels: Vec<&str> = pile
            .groups
            .iter()
            .map(|(_, label)| label.as_str())
            .collect();
        assert_eq!(labels, ["Forward strand", "Reverse strand"]);
        assert_eq!(pile.rows, [1, 0]);
    }

    #[test]
    fn sorting_keeps_rows_until_new_reads_load() {
        let mut tracks = three();
        let reads = vec![
            alignment(10, vec![(Op::Match, 10)]),
            alignment(12, vec![(Op::Match, 10)]),
        ];
        let wide = region(0, 100);
        let loaded = Loaded::reads(wide, 0, reads, vec![b'A'; 100], 0, Options::default());
        tracks[1].wanted = Some((0, wide));
        tracks[1].accept(0, wide, Ok(Arc::new(loaded)));
        change(&mut tracks, 1, Change::Sort(15));
        assert_eq!(tracks[1].order.as_ref().map(|(at, _)| *at), Some(15));
        tracks[1].wanted = Some((0, wide));
        tracks[1].accept(0, wide, Ok(loaded_reads(wide)));
        assert_eq!(tracks[1].order, None);
    }

    fn loaded_reads(wide: Region) -> Arc<Loaded> {
        let reads = vec![alignment(30, vec![(Op::Match, 10)])];
        Arc::new(Loaded::reads(
            wide,
            0,
            reads,
            vec![b'A'; 100],
            0,
            Options::default(),
        ))
    }

    #[test]
    fn loaded_reads_mark_unusual_pairs() {
        let pair = |start: u64, contig: usize, mate: u64, insert: i64| Alignment {
            flags: PAIRED | PROPER | MATE_REVERSE,
            mate: Some(Mate {
                contig,
                start: mate,
                insert,
            }),
            ..alignment(start, vec![(Op::Match, 10)])
        };
        let mut alignments: Vec<Alignment> = (0..150)
            .map(|index| pair(index, 0, index + 300, 300 + index.cast_signed() % 50))
            .collect();
        alignments.push(Alignment {
            flags: PAIRED | MATE_REVERSE,
            ..pair(160, 0, 9_000, 8_850)
        });
        alignments.push(pair(170, 1, 500, 0));
        let options = Options {
            downsample: false,
            ..Options::default()
        };
        let loaded = Loaded::reads(
            region(0, 1_000),
            0,
            alignments,
            vec![b'A'; 1_000],
            0,
            options,
        );
        let Content::Reads(pile) = &loaded.content else {
            panic!("expected reads");
        };
        let classes: Vec<Pairing> = pile.reads[150..].iter().map(|read| read.pairing).collect();
        assert_eq!(classes, [Pairing::Large, Pairing::Elsewhere]);
        assert_eq!(pile.reads[0].pairing, Pairing::Normal);
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
        let loaded = load(
            &source,
            "chr1",
            window,
            Options::default(),
            1,
            true,
            no_reference,
        )
        .unwrap();
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
    fn opens_bgzip_vcf_with_a_tbi_or_a_csi() {
        for suffix in ["tbi", "csi"] {
            let scratch = Scratch::new(&format!("vcf-{suffix}"));
            let data = std::fs::read(fixture("variants.vcf.gz")).unwrap();
            let path = scratch.file("calls.vcf.gz", data);
            let index = std::fs::read(fixture(&format!("variants.vcf.gz.{suffix}"))).unwrap();
            scratch.file(&format!("calls.vcf.gz.{suffix}"), index);
            let source = open(path).unwrap();
            assert_eq!(source.samples(), ["S1", "S2", "S3"]);
            assert_eq!(source.span(), 1_000_000);
            let window = region(1_000, 60_000);
            let loaded = load(
                &source,
                "chr1",
                window,
                Options::default(),
                1,
                true,
                no_reference,
            )
            .unwrap();
            let Content::Variants(calls) = &loaded.content else {
                panic!("expected variants");
            };
            assert!(calls.variants.len() > 10, "{}", calls.variants.len());
            assert_eq!(loaded.count(), 3);
            let names = names(&source, &assembly()).unwrap();
            assert!(names.find("rs1001").is_some() || names.find("rs1002").is_some());
        }
    }

    #[test]
    fn opens_small_plain_vcf_files() {
        let scratch = Scratch::new("plain-vcf");
        let text = "#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\tFORMAT\tA\n\
                    chr1\t20\trs9\tA\tG\t.\tPASS\t.\tGT\t0/1\n";
        let source = open(scratch.file("calls.vcf", text)).unwrap();
        assert!(matches!(source.format, Format::VariantTable { .. }));
        assert_eq!(source.contigs(), ["chr1"]);
        let window = region(0, 100);
        let loaded = load(
            &source,
            "chr1",
            window,
            Options::default(),
            1,
            true,
            no_reference,
        )
        .unwrap();
        assert_eq!(loaded.count(), 1);
        assert_eq!(
            names(&source, &assembly())
                .unwrap()
                .find("rs9")
                .unwrap()
                .contig,
            0
        );
    }

    fn signal(loaded: &Loaded) -> &[genie_core::signal::Summary] {
        let Content::Signal(signal) = &loaded.content else {
            panic!("expected signal");
        };
        &signal.summaries
    }

    #[test]
    fn knows_signal_files_by_name() {
        for name in [
            "a.bw",
            "B.BigWig",
            "c.bedGraph",
            "d.bdg",
            "e.bedgraph.gz",
            "f.bdg.bgz",
        ] {
            assert!(is_track(&PathBuf::from(name)), "{name}");
        }
        assert!(!is_track(&PathBuf::from("g.wig")));
        assert!(is_track(&PathBuf::from("genes.gtf.gz")));
    }

    #[test]
    fn steps_follow_the_zoom_in_powers_of_four() {
        let found: Vec<u64> = [0.5, 2.0, 8.0, 10.0, 32.0, 1_000.0]
            .into_iter()
            .map(step)
            .collect();
        assert_eq!(found, [1, 1, 4, 4, 16, 256]);
    }

    #[test]
    fn opens_bigwig_and_loads_the_zoom_level_for_the_step() {
        let source = open(fixture("signal.bw")).unwrap();
        assert!(source.has_signal());
        assert_eq!(source.span(), u64::MAX);
        assert!(source.contigs().contains(&"chrX".to_owned()));
        let fine = load(
            &source,
            "chr1",
            region(0, 5_000),
            Options::default(),
            1,
            true,
            no_reference,
        );
        assert_eq!(signal(&fine.unwrap()).len(), 9);
        let whole = region(0, 3_000_000);
        let coarse = load(
            &source,
            "chr1",
            whole,
            Options::default(),
            16_384,
            true,
            no_reference,
        );
        let count = signal(&coarse.unwrap()).len();
        assert!((1..1_000).contains(&count), "{count}");
        assert_eq!(names(&source, &assembly()).unwrap(), Names::default());
    }

    #[test]
    fn opens_bigbed_with_features_up_close_and_density_far_out() {
        let source = open(fixture("genes.bb")).unwrap();
        assert!(matches!(source.format, Format::BigBed { .. }));
        assert!(source.has_colors() && !source.has_signal());
        assert_eq!(source.span(), u64::MAX);
        let near = region(0, 20_000);
        let close = load(
            &source,
            "chr1",
            near,
            Options::default(),
            1,
            true,
            no_reference,
        )
        .unwrap();
        let Content::Features(annotation) = &close.content else {
            panic!("expected features");
        };
        assert_eq!(annotation.features[0].name(), "chr1_001");
        assert_eq!(annotation.features[0].color(), Some([200, 40, 40]));
        let whole = region(0, 3_000_000);
        let far = load(
            &source,
            "chr1",
            whole,
            Options::default(),
            16_384,
            false,
            no_reference,
        );
        assert_ne!(signal(&far.unwrap()), []);
        assert_eq!(names(&source, &assembly()).unwrap(), Names::default());
        assert!(is_track(&PathBuf::from("peaks.bb")) && is_track(&PathBuf::from("a.bigBed")));
    }

    #[test]
    fn bigbed_tracks_reload_when_the_view_crosses_ten_megabases() {
        let source = open(fixture("genes.bb")).unwrap();
        let mut track = Track::new(5, Arc::new(source), &assembly());
        track.contigs = vec![Some("chr1".to_owned()), None, None];
        let wide = region(0, 1_000_000);
        track.wanted = Some((0, wide));
        let features = Loaded::features(wide, Vec::new());
        track.accept(0, wide, Ok(Arc::new(features)));
        assert!(!track.needs(0, region(100, 200), 1));
        assert!(!track.needs(0, region(0, 900_000), 1));
        let mut far = Track::new(6, Arc::clone(&track.source), &assembly());
        far.contigs = track.contigs.clone();
        far.wanted = Some((0, wide));
        far.accept(0, wide, Ok(Arc::new(Loaded::signal(wide, Vec::new(), 4))));
        assert!(far.needs(0, region(100, 200), 4));
    }

    #[test]
    fn opens_plain_and_indexed_bedgraph() {
        let scratch = Scratch::new("bedgraph");
        let text = "track type=bedGraph\nchr1\t0\t10\t1.5\nchr1\t10\t20\t2.5\n";
        let plain = open(scratch.file("coverage.bedGraph", text)).unwrap();
        assert!(matches!(plain.format, Format::BedGraphTable { .. }));
        assert_eq!(plain.span(), u64::MAX);
        let window = region(0, 100);
        let raw = load(
            &plain,
            "chr1",
            window,
            Options::default(),
            1,
            true,
            no_reference,
        )
        .unwrap();
        assert_eq!(signal(&raw).len(), 2);
        let reduced = load(
            &plain,
            "chr1",
            window,
            Options::default(),
            16,
            true,
            no_reference,
        )
        .unwrap();
        let spans: Vec<Region> = signal(&reduced)
            .iter()
            .map(|summary| summary.span)
            .collect();
        assert_eq!(spans, [region(0, 16), region(16, 20)]);
        let data = std::fs::read(fixture("plain.bedGraph.gz")).unwrap();
        let path = scratch.file("coverage.bedGraph.gz", data);
        let index = std::fs::read(fixture("plain.bedGraph.gz.tbi")).unwrap();
        scratch.file("coverage.bedGraph.gz.tbi", index);
        let indexed = open(path).unwrap();
        assert!(matches!(indexed.format, Format::BedGraph { .. }));
        assert_eq!(indexed.span(), 10_000_000);
        let window = region(0, 10_000);
        let loaded = load(
            &indexed,
            "chr2",
            window,
            Options::default(),
            1,
            true,
            no_reference,
        )
        .unwrap();
        assert_ne!(signal(&loaded), []);
    }

    #[test]
    fn signal_tracks_reload_when_the_step_gets_finer() {
        let source = Source {
            path: PathBuf::from("/data/coverage.bedGraph"),
            name: "coverage.bedGraph".to_owned(),
            format: Format::BedGraphTable {
                table: genie_formats::table::Table::new(Vec::new()),
            },
        };
        let mut track = Track::new(3, Arc::new(source), &assembly());
        track.contigs = vec![Some("chr1".to_owned()), None, None];
        let wide = region(0, 1_000);
        track.wanted = Some((0, wide));
        track.accept(0, wide, Ok(Arc::new(Loaded::signal(wide, Vec::new(), 16))));
        let shown = region(100, 200);
        assert!(!track.needs(0, shown, 16));
        assert!(!track.needs(0, shown, 64));
        assert!(track.needs(0, shown, 4));
    }

    #[test]
    fn vcf_files_ask_for_an_index() {
        let scratch = Scratch::new("vcf-no-index");
        let data = std::fs::read(fixture("variants.vcf.gz")).unwrap();
        let error = open(scratch.file("calls.vcf.gz", data)).unwrap_err();
        assert_eq!(
            error,
            "There is no index calls.vcf.gz.tbi. Index the file with tabix."
        );
    }

    #[test]
    fn opens_small_plain_feature_files() {
        let scratch = Scratch::new("plain");
        let text = "chr1\t10\t20\tgeneA\t0\t+\nchr2\t5\t9\tgeneB\t0\t-\n";
        let source = open(scratch.file("genes.bed", text)).unwrap();
        assert!(matches!(
            source.format,
            Format::Table {
                kind: Kind::Bed,
                ..
            }
        ));
        assert_eq!(source.contigs(), ["chr1", "chr2"]);
        let loaded = load(
            &source,
            "chr1",
            region(0, 100),
            Options::default(),
            1,
            true,
            no_reference,
        )
        .unwrap();
        assert_eq!(loaded.count(), 1);
        let names = names(&source, &assembly()).unwrap();
        assert_eq!(names.find("geneb").unwrap().contig, 1);
    }

    #[test]
    fn large_plain_files_need_tabix() {
        assert_eq!(fits("genes.bed", 50 << 20), Ok(()));
        assert_eq!(
            fits("genes.bed", (50 << 20) + 1),
            Err(
                "The file genes.bed is larger than 50 MB. Compress it with bgzip, then index it with tabix."
                    .to_owned()
            )
        );
    }

    #[test]
    fn plain_files_report_bad_lines() {
        let scratch = Scratch::new("plain-bad");
        let error = open(scratch.file("genes.bed", "chr1\tx\t9\tbad\n")).unwrap_err();
        assert_eq!(
            error,
            "The file genes.bed is not valid. Line 1: a number is not valid."
        );
    }

    #[test]
    fn opens_and_loads_reads_made_by_samtools() {
        let source = open(fixture("reads.bam")).unwrap();
        assert_eq!(source.contigs(), ["chr1", "chr2"]);
        let reference = |window: Region| Ok(vec![b'A'; window.len() as usize]);
        let window = region(5_000, 8_000);
        let loaded = load(
            &source,
            "chr1",
            window,
            Options::default(),
            1,
            true,
            reference,
        )
        .unwrap();
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
            mate: None,
            tags: genie_core::alignment::Tags::default(),
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
        assert!(!track.needs(0, region(100, 200), 1));
        track.options = track.options.toggled(Toggle::Clips);
        assert!(track.needs(0, region(100, 200), 1));
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
            let loaded =
                Loaded::reads(region(0, 100), 0, deep.clone(), vec![b'A'; 100], 0, options);
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
        let loaded = load(
            &source,
            "chr1",
            region(1_000, 41_000),
            options,
            1,
            true,
            reference,
        )
        .unwrap();
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
        let loaded = Loaded::reads(
            region(0, 1_000),
            0,
            alignments,
            vec![b'A'; 1_000],
            0,
            options,
        );
        let Content::Reads(pile) = &loaded.content else {
            panic!("expected reads");
        };
        assert_eq!(pile.count, 3);
        let (rows, count, _) = pile.packed(region(0, 200));
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
        assert!(track.needs(0, region(100, 200), 1));
        assert!(!track.needs(1, region(100, 200), 1));
        assert!(!track.needs(0, region(0, 20_000_000), 1));
        track.wanted = Some((0, region(0, 300)));
        assert!(!track.needs(0, region(100, 200), 1));
        assert!(track.needs(0, region(250, 350), 1));
    }

    #[test]
    fn accepts_only_the_latest_request() {
        let mut track = track();
        track.wanted = Some((0, region(0, 300)));
        track.accept(0, region(0, 100), Ok(loaded(region(0, 100), 1)));
        assert!(track.data(0).is_none());
        track.accept(0, region(0, 300), Ok(loaded(region(0, 300), 1)));
        assert_eq!(track.data(0).unwrap().region, region(0, 300));
        assert!(!track.needs(0, region(100, 200), 1));
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
        let error = open(PathBuf::from("/data/calls.bcf")).unwrap_err();
        assert_eq!(
            error,
            "The file calls.bcf is not a BED, GFF3, GTF, VCF, BAM, bigWig, bigBed or bedGraph file."
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
