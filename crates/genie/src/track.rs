use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use genie_core::alignment::{Alignment, Inserts, REVERSE};
use genie_core::pileup::{Coverage, Read, Sampler, sort_rows};
use genie_core::signal::Summary;
use genie_core::variant::Variant;
use genie_core::{Assembly, Feature, Named, Names, Region};
use genie_formats::features::Kind;
use genie_formats::fields::{self, Field, Rule};
use genie_formats::{bbi, bed, gff, tabix, vcf};
use genie_render::features::Mode;
use genie_render::scene::Hue;
use genie_render::signal::{Scale, Style};
use genie_render::{alignments, rows};

use crate::location::{Location, Reader};

mod features;
mod reads;
mod signal;
mod variants;

pub use features::FeatureFile;
pub use reads::ReadFile;
pub use signal::SignalFile;
pub use variants::VariantFile;

const FEATURE_SPAN: u64 = 10_000_000;
const WINDOW: u64 = 50;
const DEPTH: usize = 100;
const READ_GAP: u64 = 2;
const PLAIN_LIMIT: u64 = 50 << 20;
const SEARCH: u64 = 1 << 20;

#[derive(Debug)]
pub enum Format {
    Features(FeatureFile),
    Variants(VariantFile),
    Reads(ReadFile),
    Signal(SignalFile),
}

#[derive(Debug)]
pub struct Source {
    pub location: Location,
    pub name: String,
    pub format: Format,
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.location == other.location
    }
}

impl Source {
    pub fn contigs(&self) -> &[String] {
        match &self.format {
            Format::Features(file) => file.contigs(),
            Format::Variants(file) => file.contigs(),
            Format::Reads(file) => &file.contigs,
            Format::Signal(file) => file.contigs(),
        }
    }

    pub fn is_bigbed(&self) -> bool {
        matches!(self.format, Format::Features(FeatureFile::BigBed { .. }))
    }

    pub fn has_lookup(&self) -> bool {
        matches!(&self.format, Format::Features(FeatureFile::BigBed { index }) if index.has_lookup())
    }

    pub fn has_colors(&self) -> bool {
        matches!(&self.format, Format::Features(file) if file.has_colors())
    }

    pub fn has_signal(&self) -> bool {
        matches!(self.format, Format::Signal(_))
    }

    pub fn has_reads(&self) -> bool {
        matches!(self.format, Format::Reads(_))
    }

    pub fn has_variants(&self) -> bool {
        matches!(self.format, Format::Variants(_))
    }

    pub fn can_jump(&self) -> bool {
        matches!(self.format, Format::Features(_) | Format::Variants(_))
    }

    pub fn samples(&self) -> &[String] {
        match &self.format {
            Format::Variants(file) => file.samples(),
            _ => &[],
        }
    }

    pub fn span(&self) -> u64 {
        match &self.format {
            Format::Features(file) => file.span(),
            Format::Variants(_) => VariantFile::SPAN,
            Format::Reads(_) => ReadFile::SPAN,
            Format::Signal(file) => file.span(),
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plot {
    pub style: Style,
    pub linked: bool,
    pub pairs: bool,
    pub colors: bool,
    pub range: Option<Scale>,
}

impl Default for Plot {
    fn default() -> Self {
        Self {
            style: Style::default(),
            linked: false,
            pairs: true,
            colors: true,
            range: None,
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

impl Annotation {
    pub fn packed(&self, visible: Region) -> (Vec<usize>, usize) {
        let shown: Vec<usize> = (0..self.features.len())
            .filter(|&index| {
                let span = self.features[index].span();
                span.start() <= visible.end() && span.end() >= visible.start()
            })
            .collect();
        let packed = rows::pack(&shown, |&index| self.features[index].span(), 0);
        let mut rows = vec![0; self.features.len()];
        for (&index, &row) in shown.iter().zip(&packed) {
            rows[index] = row;
        }
        (rows, count(&packed))
    }
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

#[derive(Debug)]
pub struct Gather {
    contig: usize,
    options: Options,
    coverage: Coverage,
    inserts: Inserts,
    sampler: Sampler,
}

impl Gather {
    pub fn new(region: Region, contig: usize, options: Options) -> Self {
        let limit = if options.downsample {
            DEPTH
        } else {
            usize::MAX
        };
        Self {
            contig,
            options,
            coverage: Coverage::new(region),
            inserts: Inserts::new(contig),
            sampler: Sampler::new(WINDOW, limit),
        }
    }

    pub fn add(&mut self, alignment: Alignment) {
        self.coverage.add(&alignment);
        self.inserts.add(&alignment);
        self.sampler.push(alignment);
    }
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

    pub fn reads(gather: Gather, reference: Vec<u8>, offset: u64) -> Self {
        let Gather {
            contig,
            options,
            coverage,
            inserts,
            sampler,
        } = gather;
        let region = coverage.region();
        let range = inserts.expected();
        let reads: Vec<Read> = sampler
            .finish()
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    pub rules: Vec<Rule>,
    pub label: Option<String>,
}

impl Filter {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.label.is_none()
    }
}

#[derive(Debug, Default)]
pub struct Slot {
    pub loaded: Option<(usize, Arc<Loaded>)>,
    pub shown: Option<Arc<Loaded>>,
    pub wanted: Option<(usize, Region)>,
    pub error: Option<String>,
    pub order: Option<(u64, Vec<usize>)>,
}

static EMPTY: Slot = Slot {
    loaded: None,
    shown: None,
    wanted: None,
    error: None,
    order: None,
};

impl Slot {
    pub fn data(&self, contig: usize) -> Option<&Loaded> {
        let (loaded, data) = self.loaded.as_ref()?;
        (*loaded == contig).then(|| self.shown.as_deref().unwrap_or(data))
    }

    pub fn rows<'a>(&'a self, pile: &'a Pile) -> &'a [usize] {
        self.order.as_ref().map_or(&pile.rows, |(_, rows)| rows)
    }
}

#[derive(Debug)]
pub struct Track {
    pub id: u64,
    pub source: Arc<Source>,
    pub name: String,
    pub over: Option<u64>,
    pub contigs: Vec<Option<String>>,
    pub names: Option<Arc<Names>>,
    pub hue: Hue,
    pub mode: Option<Mode>,
    pub options: Options,
    pub plot: Plot,
    pub height: Option<f32>,
    pub filter: Filter,
    pub slot: Slot,
    pub extra: Vec<Slot>,
}

impl Track {
    pub fn new(id: u64, source: Arc<Source>, assembly: &Assembly) -> Self {
        let contigs = contig_names(source.contigs(), assembly);
        Self {
            id,
            name: source.name.clone(),
            over: None,
            source,
            contigs,
            names: None,
            hue: Hue::default(),
            mode: None,
            options: Options::default(),
            plot: Plot::default(),
            height: None,
            filter: Filter::default(),
            slot: Slot::default(),
            extra: Vec::new(),
        }
    }

    pub fn refilter(&mut self) {
        let shown = self
            .slot
            .loaded
            .as_ref()
            .and_then(|(_, data)| self.derive(data));
        self.slot.shown = shown;
        for index in 0..self.extra.len() {
            let shown = self.extra[index]
                .loaded
                .as_ref()
                .and_then(|(_, data)| self.derive(data));
            self.extra[index].shown = shown;
        }
    }

    fn derive(&self, data: &Loaded) -> Option<Arc<Loaded>> {
        if self.filter.is_empty() {
            return None;
        }
        let content = match &data.content {
            Content::Features(annotation) => {
                let kept = annotation
                    .features
                    .iter()
                    .filter_map(|feature| self.kept(feature))
                    .collect();
                Loaded::features(data.region, kept).content
            }
            Content::Variants(calls) => {
                let rules = &self.filter.rules;
                let variants = calls
                    .variants
                    .iter()
                    .filter(|variant| fields::passes(rules, &variant_fields(&self.source, variant)))
                    .cloned()
                    .collect();
                Content::Variants(Calls {
                    variants,
                    samples: calls.samples,
                })
            }
            Content::Reads(_) | Content::Signal(_) => return None,
        };
        Some(Arc::new(Loaded {
            region: data.region,
            options: data.options,
            content,
        }))
    }

    fn kept(&self, feature: &Feature) -> Option<Feature> {
        let fields = feature_fields(&self.source, feature);
        if !fields::passes(&self.filter.rules, &fields) {
            return None;
        }
        let label = self
            .filter
            .label
            .as_ref()
            .and_then(|label| fields.iter().find(|field| &field.label == label));
        Some(match label {
            Some(field) => feature.clone().with_name(field.value.clone()),
            None => feature.clone(),
        })
    }

    pub fn slot(&self, panel: usize) -> &Slot {
        match panel {
            0 => &self.slot,
            _ => self.extra.get(panel - 1).unwrap_or(&EMPTY),
        }
    }

    pub fn slot_mut(&mut self, panel: usize) -> &mut Slot {
        if panel == 0 {
            return &mut self.slot;
        }
        if self.extra.len() < panel {
            self.extra.resize_with(panel, Slot::default);
        }
        &mut self.extra[panel - 1]
    }

    pub fn forget(&mut self) {
        self.slot.wanted = None;
        for slot in &mut self.extra {
            slot.wanted = None;
        }
    }

    pub fn data(&self, contig: usize) -> Option<&Loaded> {
        self.slot.data(contig)
    }

    pub fn mode_at(&self, panel: usize, contig: usize) -> Mode {
        self.mode.unwrap_or_else(|| {
            let count = self.slot(panel).data(contig).map_or(0, Loaded::count);
            if self.source.has_reads() {
                alignments::fit(count)
            } else if self.source.has_variants() {
                genie_render::variants::fit(count)
            } else {
                Mode::fit(count)
            }
        })
    }

    pub fn needs_at(&self, panel: usize, contig: usize, visible: Region, step: u64) -> bool {
        let slot = self.slot(panel);
        let covers = |(wanted, region): (usize, Region)| {
            wanted == contig && region.start() <= visible.start() && visible.end() <= region.end()
        };
        let fits = |data: &Loaded| {
            !self.source.is_bigbed()
                || matches!(data.content, Content::Features(_)) == detailed(visible)
        };
        let loaded = slot
            .data(contig)
            .filter(|data| data.options == self.options && data.step() <= step && fits(data))
            .map(|data| (contig, data.region));
        self.contigs.get(contig).is_some_and(Option::is_some)
            && visible.len() <= self.source.span()
            && !loaded.is_some_and(covers)
            && !slot.wanted.is_some_and(covers)
    }

    pub fn accept_at(
        &mut self,
        panel: usize,
        contig: usize,
        region: Region,
        result: Result<Arc<Loaded>, String>,
    ) {
        let stale = result
            .as_ref()
            .is_ok_and(|data| data.options != self.options);
        if self.slot(panel).wanted != Some((contig, region)) || stale {
            return;
        }
        let shown = result.as_ref().ok().and_then(|data| self.derive(data));
        let slot = self.slot_mut(panel);
        slot.wanted = None;
        match result {
            Ok(data) => {
                slot.loaded = Some((contig, data));
                slot.shown = shown;
                slot.error = None;
                slot.order = None;
            }
            Err(error) => slot.error = Some(error),
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
    Sort(usize, u64),
    Group(Grouping),
    Up,
    Down,
    Remove,
    Overlay(u64),
    Separate,
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
            if plot.linked {
                plot.range = None;
            }
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
            track.forget();
        }
        Change::Sort(panel, at) => {
            let slot = tracks[position].slot_mut(panel);
            slot.order = slot
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
            track.forget();
        }
        Change::Up if position > 0 => tracks.swap(position, position - 1),
        Change::Down if position + 1 < tracks.len() => tracks.swap(position, position + 1),
        Change::Up | Change::Down => {}
        Change::Remove => {
            tracks.remove(position);
            free(tracks, id);
        }
        Change::Overlay(base) => overlay(tracks, position, base),
        Change::Separate => {
            if tracks[position].over.take().is_none() {
                free(tracks, id);
            }
        }
    }
}

fn free(tracks: &mut [Track], base: u64) {
    for track in tracks.iter_mut().filter(|track| track.over == Some(base)) {
        track.over = None;
    }
}

fn overlay(tracks: &mut [Track], position: usize, base: u64) {
    let id = tracks[position].id;
    let fits = tracks[position].source.has_signal()
        && !tracks.iter().any(|track| track.over == Some(id))
        && tracks.iter().any(|track| {
            track.id == base && track.id != id && track.over.is_none() && track.source.has_signal()
        });
    if !fits {
        return;
    }
    let used: Vec<Hue> = tracks
        .iter()
        .filter(|track| track.id != id && (track.id == base || track.over == Some(base)))
        .map(|track| track.hue)
        .collect();
    let track = &mut tracks[position];
    track.over = Some(base);
    if used.contains(&track.hue)
        && let Some(free) = Hue::ALL.into_iter().find(|hue| !used.contains(hue))
    {
        track.hue = free;
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

pub fn is_track(location: &Location) -> bool {
    let name = location.name();
    is_bam(&name)
        || is_vcf(&name)
        || is_signal(&name)
        || is_bigbed(&name)
        || Kind::of(&name).is_some()
}

fn is_bam(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bam"))
}

fn inner_extension(name: &str, wanted: &[&str]) -> bool {
    let path = Path::new(name);
    let stem = if is_compressed(name) {
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

fn is_compressed(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("gz") || extension.eq_ignore_ascii_case("bgz")
    })
}

fn plain(location: &Location, name: &str) -> Result<BufReader<Reader>, String> {
    fits(name, location.size()?)?;
    location.open().map(BufReader::new)
}

fn tabix_index(location: &Location, name: &str) -> Result<tabix::Index, String> {
    let (file, suffix) = [".tbi", ".csi"]
        .into_iter()
        .find_map(|suffix| Some((location.with_suffix(suffix).open().ok()?, suffix)))
        .ok_or_else(|| format!("There is no index {name}.tbi. Index the file with tabix."))?;
    tabix::Index::read(file)
        .map_err(|error| format!("The index {name}{suffix} is not valid. {error}"))
}

pub fn open(location: impl Into<Location>) -> Result<Source, String> {
    let location = location.into();
    let name = location.name();
    let format = if is_bam(&name) {
        Format::Reads(ReadFile::open(&location, &name)?)
    } else if is_vcf(&name) {
        Format::Variants(VariantFile::open(&location, &name)?)
    } else if is_bigbed(&name) || is_bigwig(&name) {
        open_bbi(
            &location,
            &name,
            crate::catalog::label(&location.to_string()),
        )?
    } else if is_signal(&name) {
        Format::Signal(SignalFile::open(&location, &name)?)
    } else {
        Format::Features(FeatureFile::open(&location, &name)?)
    };
    Ok(Source {
        location,
        name,
        format,
    })
}

fn open_bbi(location: &Location, name: &str, label: Option<&str>) -> Result<Format, String> {
    let mut file = location.open()?;
    let mut index = bbi::Index::read(&mut file)
        .map_err(|error| format!("The file {name} is not valid. {error}"))?;
    if let Some(field) = label {
        index.label_with(field);
    }
    Ok(if index.is_bed() {
        Format::Features(FeatureFile::BigBed { index })
    } else {
        Format::Signal(SignalFile::BigWig { index })
    })
}

fn fits(name: &str, size: u64) -> Result<(), String> {
    if size > PLAIN_LIMIT {
        return Err(format!(
            "The file {name} is larger than 50 MB. Compress it with bgzip, then index it with tabix."
        ));
    }
    Ok(())
}

pub fn feature_fields(source: &Source, feature: &Feature) -> Vec<Field> {
    match &source.format {
        Format::Features(FeatureFile::BigBed { index }) => index.fields(feature.extra()),
        Format::Features(FeatureFile::Indexed { kind, .. } | FeatureFile::Table { kind, .. }) => {
            match kind {
                Kind::Bed => bed::fields(feature.extra()),
                Kind::Gff | Kind::Gtf => gff::fields(feature.extra()),
            }
        }
        _ => Vec::new(),
    }
}

pub fn variant_fields(source: &Source, variant: &Variant) -> Vec<Field> {
    match &source.format {
        Format::Variants(file) => vcf::fields(&variant.info, file.infos()),
        _ => Vec::new(),
    }
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
    let location = &source.location;
    match &source.format {
        Format::Features(file) => file.load(location, contig, region, step, detail),
        Format::Variants(file) => file.load(location, contig, region),
        Format::Reads(file) => file.load(location, contig, region, options, reference),
        Format::Signal(file) => file.load(location, contig, region, step),
    }
}

pub fn neighbor(
    source: &Source,
    names: &[Option<String>],
    assembly: &Assembly,
    contig: usize,
    position: u64,
    forward: bool,
) -> Result<Option<(usize, Region)>, String> {
    let count = assembly.contigs().len().min(names.len());
    if !source.can_jump() || contig >= count {
        return Ok(None);
    }
    let order: Vec<usize> = if forward {
        (contig..count).collect()
    } else {
        (0..=contig).rev().collect()
    };
    for index in order {
        let Some(name) = names[index].as_deref() else {
            continue;
        };
        let length = assembly.contigs()[index].length;
        let threshold = (index == contig).then_some(position);
        if let Some(span) = nearest(source, name, length, threshold, forward)? {
            return Ok(Some((index, span)));
        }
    }
    Ok(None)
}

fn nearest(
    source: &Source,
    name: &str,
    length: u64,
    threshold: Option<u64>,
    forward: bool,
) -> Result<Option<Region>, String> {
    let key = |span: &Region| (span.start(), span.end());
    let mut size = SEARCH;
    if forward {
        let mut low = threshold.map_or(0, |position| position + 1);
        while low < length {
            let high = low.saturating_add(size).min(length);
            let found = spans(source, name, Region::between(low, high))?
                .into_iter()
                .filter(|span| span.start() >= low && span.start() < high)
                .min_by_key(key);
            if found.is_some() {
                return Ok(found);
            }
            (low, size) = (high, size.saturating_mul(2));
        }
    } else {
        let mut high = threshold.unwrap_or(length).min(length);
        while high > 0 {
            let low = high.saturating_sub(size);
            let found = spans(source, name, Region::between(low, high))?
                .into_iter()
                .filter(|span| span.start() >= low && span.start() < high)
                .max_by_key(key);
            if found.is_some() {
                return Ok(found);
            }
            (high, size) = (low, size.saturating_mul(2));
        }
    }
    Ok(None)
}

fn spans(source: &Source, contig: &str, region: Region) -> Result<Vec<Region>, String> {
    match &source.format {
        Format::Features(file) => file.spans(&source.location, contig, region),
        Format::Variants(file) => file.spans(&source.location, contig, region),
        Format::Reads(_) | Format::Signal(_) => Ok(Vec::new()),
    }
}

pub fn lookup(sources: &[Arc<Source>], name: &str, assembly: &Assembly) -> Option<Named> {
    let name = name.trim();
    sources.iter().find_map(|source| {
        let Format::Features(file) = &source.format else {
            return None;
        };
        file.find(&source.location, name)
            .into_iter()
            .find_map(|(contig, feature)| {
                Some(Named {
                    name: feature.name().to_owned(),
                    contig: assembly.find(&contig)?,
                    span: feature.span(),
                })
            })
    })
}

pub fn names(source: &Source, assembly: &Assembly) -> Result<Names, String> {
    if source.location.is_remote() {
        return Ok(Names::default());
    }
    let entries = match &source.format {
        Format::Features(file) => file.names(&source.location)?,
        Format::Variants(file) => file.names(&source.location)?,
        Format::Reads(_) | Format::Signal(_) => return Ok(Names::default()),
    };
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
        Change, Content, FeatureFile, Format, Gather, Grouping, Loaded, Options, Plot, SignalFile,
        Source, Toggle, Track, VariantFile, change, contig_names, fits, is_track, load, lookup,
        names, neighbor, open, open_bbi, spans, step,
    };
    use crate::location::Location;
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
            location: PathBuf::from("/data/genes.bed.gz").into(),
            name: "genes.bed.gz".to_owned(),
            format: Format::Features(FeatureFile::Indexed {
                kind: Kind::Bed,
                index: tabix::Index::default(),
            }),
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
        tracks[1].slot.wanted = Some((0, region(0, 10)));
        change(&mut tracks, 1, Change::Pairs);
        assert!(!tracks[1].plot.pairs);
        assert_eq!(tracks[1].slot.wanted, Some((0, region(0, 10))));
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
        let loaded = piled(region(0, 100), reads, options);
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
        tracks[1].slot.wanted = Some((0, region(0, 10)));
        change(&mut tracks, 1, Change::Group(Grouping::Phase));
        assert_eq!(tracks[1].options.group, Grouping::Phase);
        assert_eq!(tracks[1].slot.wanted, None);
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
        let loaded = piled(region(0, 100), reads, options);
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
        let loaded = piled(wide, reads, Options::default());
        tracks[1].slot.wanted = Some((0, wide));
        tracks[1].accept_at(0, 0, wide, Ok(Arc::new(loaded)));
        change(&mut tracks, 1, Change::Sort(0, 15));
        assert_eq!(tracks[1].slot.order.as_ref().map(|(at, _)| *at), Some(15));
        tracks[1].slot.wanted = Some((0, wide));
        tracks[1].accept_at(0, 0, wide, Ok(loaded_reads(wide)));
        assert_eq!(tracks[1].slot.order, None);
    }

    fn loaded_reads(wide: Region) -> Arc<Loaded> {
        let reads = vec![alignment(30, vec![(Op::Match, 10)])];
        Arc::new(piled(wide, reads, Options::default()))
    }

    fn piled(region: Region, alignments: Vec<Alignment>, options: Options) -> Loaded {
        let mut gather = Gather::new(region, 0, options);
        for alignment in alignments {
            gather.add(alignment);
        }
        Loaded::reads(gather, vec![b'A'; region.end() as usize], 0)
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
        let loaded = piled(region(0, 1_000), alignments, options);
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
            Format::Features(FeatureFile::Indexed {
                kind: Kind::Gff,
                ..
            })
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
    fn a_label_field_names_big_gene_pred_features() {
        let location = Location::File(fixture("genes.pred.bb"));
        let names = |label: Option<&str>| -> Vec<String> {
            let source = Source {
                location: location.clone(),
                name: "genes.pred.bb".to_owned(),
                format: open_bbi(&location, "genes.pred.bb", label).unwrap(),
            };
            let window = region(0, 10_000);
            let options = Options::default();
            let loaded = load(&source, "chr1", window, options, 1, true, no_reference).unwrap();
            let Content::Features(annotation) = &loaded.content else {
                panic!("expected features");
            };
            let features = annotation.features.iter();
            features.map(|feature| feature.name().to_owned()).collect()
        };
        assert_eq!(names(None), ["GENEA", "GENEB", "GENEC"]);
        assert_eq!(names(Some("geneName")), ["ENSG01", "ENSG02", "ENSG03"]);
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
        assert!(matches!(
            source.format,
            Format::Variants(VariantFile::Table { .. })
        ));
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
            assert!(is_track(&PathBuf::from(name).into()), "{name}");
        }
        assert!(!is_track(&PathBuf::from("g.wig").into()));
        assert!(is_track(&PathBuf::from("genes.gtf.gz").into()));
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
        assert!(matches!(
            source.format,
            Format::Features(FeatureFile::BigBed { .. })
        ));
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
        assert!(
            is_track(&PathBuf::from("peaks.bb").into())
                && is_track(&PathBuf::from("a.bigBed").into())
        );
    }

    fn three_contigs() -> Assembly {
        let contigs = ["chr1", "chr2", "chrX"].map(|name| Contig {
            name: name.to_owned(),
            length: 3_000_000,
        });
        Assembly::new(contigs.into())
    }

    fn scanned(
        source: &Source,
        names: &[Option<String>],
        contig: usize,
        position: u64,
        forward: bool,
    ) -> Option<(usize, Region)> {
        let order: Vec<usize> = if forward {
            (contig..3).collect()
        } else {
            (0..=contig).rev().collect()
        };
        order.into_iter().find_map(|index| {
            let name = names[index].as_deref()?;
            let all = spans(source, name, region(0, 3_000_000)).unwrap();
            let after = all.into_iter().filter(|span| {
                index != contig || (span.start() > position) == forward && span.start() != position
            });
            let key = |span: &Region| (span.start(), span.end());
            let best = if forward {
                after.min_by_key(key)
            } else {
                after.max_by_key(key)
            };
            best.map(|span| (index, span))
        })
    }

    #[test]
    fn neighbors_match_a_scan_of_each_contig() {
        let assembly = three_contigs();
        for file in [
            "genes.bed.gz",
            "genes.gff3.gz",
            "genes.bb",
            "variants.vcf.gz",
        ] {
            let source = open(fixture(file)).unwrap();
            let names = contig_names(source.contigs(), &assembly);
            for contig in 0..3 {
                for position in [0, 1_000, 123_456, 1_500_000, 2_999_999] {
                    for forward in [true, false] {
                        let found = neighbor(&source, &names, &assembly, contig, position, forward)
                            .unwrap();
                        let expected = scanned(&source, &names, contig, position, forward);
                        assert_eq!(found, expected, "{file} {contig} {position} {forward}");
                    }
                }
            }
        }
    }

    #[test]
    fn neighbors_start_after_the_position_and_cross_contigs() {
        let assembly = three_contigs();
        let genes = open(fixture("genes.bed.gz")).unwrap();
        let names = contig_names(genes.contigs(), &assembly);
        let next = |position, forward| neighbor(&genes, &names, &assembly, 0, position, forward);
        assert_eq!(next(0, true).unwrap(), Some((0, region(1_050, 3_208))));
        let after = next(1_050, true).unwrap().unwrap();
        assert_eq!((after.0, after.1.start()), (0, 1_415));
        assert_eq!(next(1_050, false).unwrap(), None);
        let calls = open(fixture("variants.vcf.gz")).unwrap();
        let names = contig_names(calls.contigs(), &assembly);
        let first = neighbor(&calls, &names, &assembly, 0, 0, true).unwrap();
        assert_eq!(first, Some((0, region(1_000, 1_001))));
        assert_eq!(
            neighbor(&calls, &names, &assembly, 2, 0, true).unwrap(),
            None
        );
        let last = neighbor(&calls, &names, &assembly, 2, 0, false)
            .unwrap()
            .unwrap();
        assert_eq!(last.0, 1);
        let reads = open(fixture("reads.bam")).unwrap();
        let names = contig_names(reads.contigs(), &assembly);
        assert_eq!(
            neighbor(&reads, &names, &assembly, 0, 0, true).unwrap(),
            None
        );
    }

    #[test]
    fn remote_tracks_load_but_skip_name_scans() {
        let files = ["genes.bed.gz", "genes.bed.gz.tbi"]
            .map(|name| (format!("/{name}"), std::fs::read(fixture(name)).unwrap()))
            .to_vec();
        let server = crate::server::serve(files);
        let remote = open(Location::parse(&server.url("/genes.bed.gz")).unwrap()).unwrap();
        let local = open(fixture("genes.bed.gz")).unwrap();
        let assembly = three_contigs();
        assert_eq!(names(&remote, &assembly).unwrap(), Names::default());
        assert_ne!(names(&local, &assembly).unwrap(), Names::default());
        let window = region(0, 200_000);
        let load = |source: &Source| {
            let loaded = load(source, "chr1", window, Options::default(), 1, true, |_| {
                Err("There is no reference.".to_owned())
            })
            .unwrap();
            let Content::Features(annotation) = loaded.content else {
                panic!("expected features");
            };
            annotation.features
        };
        assert_ne!(load(&local).len(), 0);
        assert_eq!(load(&remote), load(&local));
    }

    #[test]
    fn bigbed_names_are_looked_up_in_the_index() {
        let source = Arc::new(open(fixture("genes.bb")).unwrap());
        assert!(source.has_lookup());
        let contigs = ["chr1", "chr2", "chrX"].map(|name| Contig {
            name: name.to_owned(),
            length: 3_000_000,
        });
        let assembly = Assembly::new(contigs.into());
        let found = lookup(&[Arc::clone(&source)], "chr2_450", &assembly).unwrap();
        assert_eq!((found.contig, found.span), (1, region(281_270, 283_541)));
        let upper = lookup(&[Arc::clone(&source)], "CHR2_450", &assembly).unwrap();
        assert_eq!(upper, found);
        assert_eq!(lookup(&[source], "missing", &assembly), None);
    }

    #[test]
    fn bigbed_tracks_reload_when_the_view_crosses_ten_megabases() {
        let source = open(fixture("genes.bb")).unwrap();
        let mut track = Track::new(5, Arc::new(source), &assembly());
        track.contigs = vec![Some("chr1".to_owned()), None, None];
        let wide = region(0, 1_000_000);
        track.slot.wanted = Some((0, wide));
        let features = Loaded::features(wide, Vec::new());
        track.accept_at(0, 0, wide, Ok(Arc::new(features)));
        assert!(!track.needs_at(0, 0, region(100, 200), 1));
        assert!(!track.needs_at(0, 0, region(0, 900_000), 1));
        let mut far = Track::new(6, Arc::clone(&track.source), &assembly());
        far.contigs = track.contigs.clone();
        far.slot.wanted = Some((0, wide));
        far.accept_at(
            0,
            0,
            wide,
            Ok(Arc::new(Loaded::signal(wide, Vec::new(), 4))),
        );
        assert!(far.needs_at(0, 0, region(100, 200), 4));
    }

    #[test]
    fn opens_plain_and_indexed_bedgraph() {
        let scratch = Scratch::new("bedgraph");
        let text = "track type=bedGraph\nchr1\t0\t10\t1.5\nchr1\t10\t20\t2.5\n";
        let plain = open(scratch.file("coverage.bedGraph", text)).unwrap();
        assert!(matches!(
            plain.format,
            Format::Signal(SignalFile::BedGraphTable { .. })
        ));
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
        assert!(matches!(
            indexed.format,
            Format::Signal(SignalFile::BedGraph { .. })
        ));
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
            location: PathBuf::from("/data/coverage.bedGraph").into(),
            name: "coverage.bedGraph".to_owned(),
            format: Format::Signal(SignalFile::BedGraphTable {
                table: genie_formats::table::Table::new(Vec::new()),
            }),
        };
        let mut track = Track::new(3, Arc::new(source), &assembly());
        track.contigs = vec![Some("chr1".to_owned()), None, None];
        let wide = region(0, 1_000);
        track.slot.wanted = Some((0, wide));
        track.accept_at(
            0,
            0,
            wide,
            Ok(Arc::new(Loaded::signal(wide, Vec::new(), 16))),
        );
        let shown = region(100, 200);
        assert!(!track.needs_at(0, 0, shown, 16));
        assert!(!track.needs_at(0, 0, shown, 64));
        assert!(track.needs_at(0, 0, shown, 4));
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
            Format::Features(FeatureFile::Table {
                kind: Kind::Bed,
                ..
            })
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
    fn plain_feature_files_need_no_second_read_for_names() {
        let scratch = Scratch::new("plain-names");
        let gff = "chr1\t.\tgene\t11\t20\t.\t+\t.\tID=g1;Name=geneA\n";
        for (name, text) in [("genes.bed", "chr1\t10\t20\tgeneA\n"), ("genes.gff3", gff)] {
            let path = scratch.file(name, text);
            let source = open(path.clone()).unwrap();
            std::fs::remove_file(&path).unwrap();
            let names = names(&source, &assembly()).unwrap();
            assert_eq!(names.find("genea").unwrap().contig, 0, "{name}");
        }
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
        tracks[1].slot.wanted = Some((0, region(0, 300)));
        change(&mut tracks, 1, Change::Toggle(Toggle::Duplicates));
        assert!(tracks[1].options.duplicates);
        assert_eq!(tracks[1].slot.wanted, None);
        change(&mut tracks, 1, Change::Toggle(Toggle::Duplicates));
        assert!(!tracks[1].options.duplicates);
    }

    #[test]
    fn data_for_other_options_is_reloaded_or_dropped() {
        let mut track = track();
        track.slot.wanted = Some((0, region(0, 300)));
        track.accept_at(0, 0, region(0, 300), Ok(loaded(region(0, 300), 1)));
        assert!(!track.needs_at(0, 0, region(100, 200), 1));
        track.options = track.options.toggled(Toggle::Clips);
        assert!(track.needs_at(0, 0, region(100, 200), 1));
        track.slot.wanted = Some((0, region(0, 300)));
        track.accept_at(0, 0, region(0, 300), Ok(loaded(region(0, 300), 2)));
        assert_eq!(track.data(0).unwrap().count(), 1);
        assert_eq!(track.slot.wanted, Some((0, region(0, 300))));
    }

    #[test]
    fn read_options_shape_the_pile() {
        let deep: Vec<Alignment> = (0..150)
            .map(|index| alignment(10 + index % 10, vec![(Op::SoftClip, 2), (Op::Match, 10)]))
            .collect();
        let pile = |options| {
            let loaded = piled(region(0, 100), deep.clone(), options);
            let Content::Reads(pile) = loaded.content else {
                panic!("expected reads");
            };
            pile
        };
        let fixed = pile(Options::default());
        assert_eq!(fixed.reads.len(), 100);
        assert_eq!(fixed.coverage.depth(19), 150);
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
        let loaded = piled(region(0, 1_000), alignments, options);
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
        assert!(track.needs_at(0, 0, region(100, 200), 1));
        assert!(!track.needs_at(0, 1, region(100, 200), 1));
        assert!(!track.needs_at(0, 0, region(0, 20_000_000), 1));
        track.slot.wanted = Some((0, region(0, 300)));
        assert!(!track.needs_at(0, 0, region(100, 200), 1));
        assert!(track.needs_at(0, 0, region(250, 350), 1));
    }

    #[test]
    fn accepts_only_the_latest_request() {
        let mut track = track();
        track.slot.wanted = Some((0, region(0, 300)));
        track.accept_at(0, 0, region(0, 100), Ok(loaded(region(0, 100), 1)));
        assert!(track.data(0).is_none());
        track.accept_at(0, 0, region(0, 300), Ok(loaded(region(0, 300), 1)));
        assert_eq!(track.data(0).unwrap().region, region(0, 300));
        assert!(!track.needs_at(0, 0, region(100, 200), 1));
        assert!(track.data(1).is_none());
    }

    #[test]
    fn each_panel_keeps_its_own_data() {
        let mut track = track();
        let wanted = region(0, 300);
        track.slot_mut(2).wanted = Some((0, wanted));
        track.accept_at(2, 0, wanted, Ok(loaded(wanted, 3)));
        assert!(track.data(0).is_none());
        assert_eq!(track.slot(2).data(0).unwrap().region, wanted);
        assert!(track.slot(1).data(0).is_none());
        assert!(track.needs_at(1, 0, region(0, 100), 1));
        assert!(!track.needs_at(2, 0, region(0, 100), 1));
        assert!(track.slot(9).data(0).is_none());
        track.slot_mut(1).wanted = Some((0, wanted));
        track.forget();
        assert_eq!(track.slot(1).wanted, None);
    }

    #[test]
    fn keeps_errors_for_the_view() {
        let mut track = track();
        track.slot.wanted = Some((0, region(0, 300)));
        track.accept_at(0, 0, region(0, 300), Err("The file is broken.".to_owned()));
        assert_eq!(track.slot.error.as_deref(), Some("The file is broken."));
    }

    #[test]
    fn fits_the_mode_to_the_rows_unless_chosen() {
        let mut track = track();
        track.slot.wanted = Some((0, region(0, 300)));
        track.accept_at(0, 0, region(0, 300), Ok(loaded(region(0, 300), 20)));
        assert_eq!(track.mode_at(0, 0), Mode::Squished);
        track.mode = Some(Mode::Collapsed);
        assert_eq!(track.mode_at(0, 0), Mode::Collapsed);
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
