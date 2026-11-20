use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use genie_core::alignment::{Alignment, Inserts, PAIRED, REVERSE, SECONDARY, SUPPLEMENTARY};
use genie_core::association::Association;
use genie_core::contact::ContactMap;
use genie_core::interaction::Interaction;
use genie_core::isoform::{Chain, Chains, Isoform};
use genie_core::methylation::{Methylation, Site, Tally};
use genie_core::pileup::{Coverage, Junction, Junctions, Read, Sampler, motif_strand, sort_rows};
use genie_core::review::{Key, Review, Reviews};
use genie_core::samples::{Arrangement, Choice, Table};
use genie_core::segment::Segment;
use genie_core::signal::{Summary, reduce as signal_reduce};
use genie_core::trio::{self, Trio};
use genie_core::variant::{Fraction, Variant};
use genie_core::{Assembly, Feature, Named, Names, Region, Strand};
use genie_formats::features::Kind;
use genie_formats::fields::{self, Field, Rule};
use genie_formats::{bbi, bed, bedmethyl, gff, gzip, tabix, vcf};
use genie_render::alignments::{self, Look, MethylationLane};
use genie_render::features::Mode;
use genie_render::rows;
use genie_render::samples::Rows;
use genie_render::scene::Hue;
use genie_render::signal::{Operation, Scale, Style};

use crate::location::{Location, Reader};

mod columns;
mod contacts;
mod features;
mod gwas;
pub mod junctions;
mod lift;
mod links;
pub mod made;
pub mod motifs;
pub(crate) mod reads;
mod segments;
mod signal;
mod variants;

pub use columns::{ColumnFile, Pick};
pub use contacts::{ContactFile, Showing};
pub use features::FeatureFile;
pub use gwas::GwasFile;
pub use junctions::JunctionFile;
pub use lift::{Lift, load_lifted};
pub use links::LinkFile;
pub use made::{Made, MadeFile};
pub use motifs::MotifFile;
pub use reads::ReadFile;
pub use segments::SegmentFile;
pub use signal::SignalFile;
pub use variants::VariantFile;

const FEATURE_SPAN: u64 = 10_000_000;
const WINDOW: u64 = 50;
const DEPTH: usize = 100;
const READ_GAP: u64 = 2;
pub const PLAIN_LIMIT: u64 = 50 << 20;
const SEARCH: u64 = 1 << 20;
const DENSITY_BUDGET: u64 = 16 << 20;
const SNIFF_LINES: usize = 50;

#[derive(Debug)]
pub enum Format {
    Features(FeatureFile),
    Variants(VariantFile),
    Reads(ReadFile),
    Signal(SignalFile),
    Links(LinkFile),
    Segments(SegmentFile),
    Contacts(ContactFile),
    Columns(ColumnFile),
    Gwas(GwasFile),
    Junctions(JunctionFile),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub data: Location,
    pub index: Location,
}

#[derive(Debug)]
pub struct Source {
    pub location: Location,
    pub data: Option<Location>,
    pub name: String,
    pub format: Format,
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.location == other.location
    }
}

impl Source {
    pub fn data(&self) -> &Location {
        self.data.as_ref().unwrap_or(&self.location)
    }

    pub fn contigs(&self) -> &[String] {
        match &self.format {
            Format::Features(file) => file.contigs(),
            Format::Variants(file) => file.contigs(),
            Format::Reads(file) => &file.contigs,
            Format::Signal(file) => file.contigs(),
            Format::Links(file) => file.contigs(),
            Format::Segments(file) => file.contigs(),
            Format::Contacts(file) => file.contigs(),
            Format::Columns(file) => file.contigs(),
            Format::Gwas(file) => file.contigs(),
            Format::Junctions(file) => file.contigs(),
        }
    }

    pub fn has_associations(&self) -> bool {
        matches!(self.format, Format::Gwas(_))
    }

    pub fn has_contacts(&self) -> bool {
        matches!(self.format, Format::Contacts(_))
    }

    pub fn has_columns(&self) -> bool {
        matches!(self.format, Format::Columns(_))
    }

    /// The normalizations of a Hi-C file, and the place of the one that a choice shows.
    pub fn normalizations(&self, choice: Option<u8>) -> (&[String], usize) {
        match &self.format {
            Format::Contacts(file) => (file.normalizations(), file.normalization(choice)),
            _ => (&[], 0),
        }
    }

    pub fn is_bigbed(&self) -> bool {
        matches!(self.format, Format::Features(FeatureFile::BigBed { .. }))
    }

    /// The motifs of a track of motif sites.
    pub fn motifs(&self) -> Option<&MotifFile> {
        match &self.format {
            Format::Features(FeatureFile::Motifs(file)) => Some(file),
            _ => None,
        }
    }

    /// What a track made from the reference shows, or nothing for the track of a file.
    pub fn made(&self) -> Option<&Made> {
        match &self.format {
            Format::Features(FeatureFile::Made(file)) | Format::Signal(SignalFile::Gc(file)) => {
                Some(&file.made)
            }
            _ => None,
        }
    }

    pub fn has_lookup(&self) -> bool {
        matches!(&self.format, Format::Features(FeatureFile::BigBed { index }) if index.has_lookup())
    }

    pub fn has_colors(&self) -> bool {
        matches!(&self.format, Format::Features(file) if file.has_colors()) || self.has_links()
    }

    pub fn has_links(&self) -> bool {
        matches!(self.format, Format::Links(_))
    }

    /// True for a file of splice junctions, such as a `SJ.out.tab` file of STAR.
    pub fn has_junctions(&self) -> bool {
        matches!(self.format, Format::Junctions(_))
    }

    pub fn has_features(&self) -> bool {
        matches!(self.format, Format::Features(_))
    }

    pub fn has_segments(&self) -> bool {
        matches!(self.format, Format::Segments(_))
    }

    /// The name of the value column of a segment file, as its header gives it.
    pub fn segment_column(&self) -> &str {
        match &self.format {
            Format::Segments(file) => &file.segments.value,
            _ => "",
        }
    }

    pub fn has_signal(&self) -> bool {
        matches!(self.format, Format::Signal(_))
    }

    pub fn is_bedmethyl(&self) -> bool {
        matches!(&self.format, Format::Signal(file) if file.is_bedmethyl())
    }

    pub fn modifications(&self) -> &[String] {
        match &self.format {
            Format::Signal(file) => file.codes(),
            _ => &[],
        }
    }

    pub fn has_reads(&self) -> bool {
        matches!(self.format, Format::Reads(_))
    }

    pub fn has_variants(&self) -> bool {
        matches!(self.format, Format::Variants(_))
    }

    /// True for the pairwise alignments of a PAF file, which a dot plot can show.
    pub fn is_paf(&self) -> bool {
        matches!(
            self.format,
            Format::Features(
                FeatureFile::Table {
                    kind: Kind::Paf,
                    ..
                } | FeatureFile::Indexed {
                    kind: Kind::Paf,
                    ..
                }
            )
        )
    }

    pub fn can_jump(&self) -> bool {
        matches!(self.format, Format::Features(_) | Format::Variants(_))
    }

    pub fn samples(&self) -> &[String] {
        match &self.format {
            Format::Variants(file) => file.samples(),
            Format::Segments(file) => file.samples(),
            _ => &[],
        }
    }

    /// The widest view that shows data.
    pub fn span(&self) -> u64 {
        match &self.format {
            Format::Features(file) => file.span(),
            Format::Variants(file) => file.span(),
            Format::Reads(_) => ReadFile::SPAN,
            Format::Signal(file) => file.span(),
            Format::Links(file) => file.span(),
            Format::Segments(_) | Format::Contacts(_) | Format::Junctions(_) => u64::MAX,
            Format::Columns(_) => FEATURE_SPAN,
            Format::Gwas(file) => file.span(),
        }
    }

    /// The widest view that shows the features or the variants of a file. Wider views show
    /// their density, if the file can give one.
    pub fn detail(&self) -> u64 {
        match &self.format {
            Format::Features(_) => FEATURE_SPAN,
            Format::Variants(_) => VariantFile::SPAN,
            _ => u64::MAX,
        }
    }

    pub fn has_density(&self) -> bool {
        self.detail() < self.span()
    }

    /// What a density of the file counts per Mb. A bigBed density shows the depth of the
    /// features instead.
    pub fn unit(&self) -> Option<&'static str> {
        match &self.format {
            Format::Variants(_) => Some("variants"),
            Format::Features(file) if !matches!(file, FeatureFile::BigBed { .. }) => {
                Some("features")
            }
            _ => None,
        }
    }

    /// Whether a view shows the features or the variants, and not their density.
    pub fn detailed(&self, visible: Region) -> bool {
        visible.len() <= self.detail()
    }
}

/// The records of each part of a region, as densities per Mb.
fn density(region: Region, step: u64, starts: impl IntoIterator<Item = u64>) -> Loaded {
    let mut tally = genie_core::signal::Tally::new(region, step);
    for start in starts {
        tally.add(start);
    }
    Loaded::signal(region, tally.summaries(), step)
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
    pub sites: Vec<Site>,
    pub kept: Option<Kept>,
}

/// The features of a zoomed-out bigBed density that the filter keeps, and all of its features.
#[derive(Debug, PartialEq)]
pub struct Kept {
    pub kept: Vec<Region>,
    pub all: Vec<Region>,
}

impl Kept {
    pub fn counts(&self, visible: Region) -> (usize, usize) {
        let count = |spans: &[Region]| {
            spans
                .iter()
                .filter(|span| span.intersection(visible).is_some())
                .count()
        };
        (count(&self.kept), count(&self.all))
    }
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
    /// The value of a tag, for example CB for the cell of a read.
    Tag([u8; 2]),
}

/// The most groups of a tag before genie puts the smallest ones together.
const TAG_GROUPS: usize = 30;

/// The name of a tag as text, for example "CB".
pub fn tag_name(name: [u8; 2]) -> String {
    String::from_utf8_lossy(&name).into_owned()
}

/// A tag name of two letters or digits, such as NM or CB.
pub fn tag(text: &str) -> Option<[u8; 2]> {
    let bytes: [u8; 2] = text.trim().as_bytes().try_into().ok()?;
    bytes.iter().all(u8::is_ascii_alphanumeric).then_some(bytes)
}

/// The text values of a tag, each with a number for its group.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Values {
    names: Vec<String>,
    numbers: HashMap<String, i64>,
}

impl Values {
    fn add(&mut self, text: &str) {
        if !self.numbers.contains_key(text) {
            let number = i64::try_from(self.names.len()).unwrap_or(i64::MAX);
            self.numbers.insert(text.to_owned(), number);
            self.names.push(text.to_owned());
        }
    }

    fn number(&self, text: &str) -> i64 {
        self.numbers.get(text).copied().unwrap_or(-1)
    }

    fn name(&self, number: i64) -> &str {
        usize::try_from(number)
            .ok()
            .and_then(|index| self.names.get(index))
            .map_or("", String::as_str)
    }
}

/// The options that change the data that a track loads. The flags are toggles of the read menu,
/// each on its own, so they stay flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Options {
    pub duplicates: bool,
    pub clips: bool,
    pub downsample: bool,
    pub group: Grouping,
    pub methyl: Methyl,
    /// Reads with a lower MAPQ stay hidden.
    pub mapq: u8,
    pub kinds: Kinds,
    /// The tag whose most common values color the reads.
    pub color: Option<[u8; 2]>,
    pub contacts: Showing,
    /// What a table with a tabix index shows.
    pub table: Pick,
    /// A number that changes each time the lift of the track changes, so that data of the
    /// earlier lift does not show.
    pub lift: u16,
    /// Whether the two mates of a pair share a row, with a line between them.
    pub mates: bool,
    /// The sample of a variant track whose B-allele frequency shows in a lane under the sites.
    pub baf: Option<usize>,
}

/// The kinds of reads that show besides primary reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kinds {
    pub secondary: bool,
    pub supplementary: bool,
    /// Reads that failed the quality checks of the sequencer (flag 0x200).
    pub failed: bool,
}

impl Default for Kinds {
    fn default() -> Self {
        Self {
            secondary: false,
            supplementary: true,
            failed: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Methyl {
    pub code: u8,
    pub separate: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            duplicates: false,
            clips: false,
            downsample: true,
            group: Grouping::None,
            methyl: Methyl::default(),
            mapq: 0,
            kinds: Kinds::default(),
            color: None,
            contacts: Showing::default(),
            table: Pick::default(),
            lift: 0,
            mates: false,
            baf: None,
        }
    }
}

/// A comparison of the values of a signal track with the values of another signal track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Math {
    /// The other track.
    pub other: u64,
    pub operation: Operation,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Plot {
    pub style: Style,
    pub linked: bool,
    pub look: Look,
    pub colors: bool,
    pub range: Option<Scale>,
    /// Whether a read track shows a Sashimi plot: its coverage and its splice junctions.
    pub sashimi: bool,
    /// Whether a read track shows its isoforms: a row for each chain of splice junctions of its
    /// reads, in place of the reads.
    pub isoforms: bool,
    /// The fewest reads of a splice junction that the Sashimi plot of a read track shows.
    pub junction_reads: u32,
    /// The comparison that a signal track shows instead of its own values.
    pub math: Option<Math>,
    /// The place of the viewpoint of the virtual 4C of a Hi-C track.
    pub viewpoint: Option<u64>,
}

impl Default for Plot {
    fn default() -> Self {
        Self {
            style: Style::default(),
            linked: false,
            look: Look {
                pairs: true,
                mods: false,
                difference: false,
            },
            colors: true,
            range: None,
            sashimi: false,
            isoforms: false,
            junction_reads: 1,
            math: None,
            viewpoint: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Duplicates,
    Clips,
    Downsample,
    Join,
    Secondary,
    Supplementary,
    Failed,
    /// The two mates of a pair on one row, with a line between them.
    Mates,
}

impl Options {
    #[must_use]
    pub fn toggled(self, toggle: Toggle) -> Self {
        let mut options = self;
        let flag = match toggle {
            Toggle::Duplicates => &mut options.duplicates,
            Toggle::Clips => &mut options.clips,
            Toggle::Downsample => &mut options.downsample,
            Toggle::Join => &mut options.methyl.separate,
            Toggle::Secondary => &mut options.kinds.secondary,
            Toggle::Supplementary => &mut options.kinds.supplementary,
            Toggle::Failed => &mut options.kinds.failed,
            Toggle::Mates => &mut options.mates,
        };
        *flag = !*flag;
        options
    }

    /// The options after a change of what a Hi-C map or a table shows, of the lowest MAPQ of
    /// the reads, or of the tag that colors the reads.
    #[must_use]
    pub fn changed(self, change: Change) -> Self {
        let mut options = self;
        match change {
            Change::Normalization(place) => options.contacts.normalization = Some(place),
            Change::Ratio => options.contacts.ratio = !options.contacts.ratio,
            Change::Column(pick) => options.table = pick,
            Change::Mapq(floor) => options.mapq = floor,
            Change::Baf(sample) => options.baf = sample,
            Change::Color(name) => {
                options.color = if options.color == Some(name) {
                    None
                } else {
                    Some(name)
                };
            }
            _ => {}
        }
        options
    }

    pub fn is_on(self, toggle: Toggle) -> bool {
        match toggle {
            Toggle::Duplicates => self.duplicates,
            Toggle::Clips => self.clips,
            Toggle::Downsample => self.downsample,
            Toggle::Join => !self.methyl.separate,
            Toggle::Secondary => self.kinds.secondary,
            Toggle::Supplementary => self.kinds.supplementary,
            Toggle::Failed => self.kinds.failed,
            Toggle::Mates => self.mates,
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
    pub junctions: Vec<Junction>,
    /// The chains of splice junctions of all reads, also the reads that down-sampling hides,
    /// and the first and the last base of each read without a junction.
    pub chains: (Vec<Chain>, Vec<Region>),
    /// The isoforms of the chains, which genie finds again only when the introns of the open
    /// transcripts change.
    pub isoforms: Memo<Vec<Isoform>>,
    pub methylation: Methylation<GroupKey>,
    pub rows: Vec<usize>,
    pub count: usize,
    pub groups: Vec<(usize, String)>,
    pub grouping: Grouping,
    /// Whether the two mates of a pair share a row.
    pub mates: bool,
    /// The text values of the tag that groups the reads.
    pub values: Values,
    /// The hue of each read when the track colors reads by a tag, or nothing.
    pub hues: Vec<Option<Hue>>,
}

/// A value that the view finds once and keeps: genie finds it again only when its key changes.
/// Two memos are equal whatever they keep, so that piles stay equal when only a memo differs.
pub struct Memo<T>(Mutex<Option<(u64, Arc<T>)>>);

impl<T> Default for Memo<T> {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

impl<T> Memo<T> {
    /// The value for a key: the kept value when the key is the same, or else a new value.
    pub fn get_or(&self, key: u64, make: impl FnOnce() -> T) -> Arc<T> {
        let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((known, value)) = kept.as_ref()
            && *known == key
        {
            return Arc::clone(value);
        }
        let value = Arc::new(make());
        *kept = Some((key, Arc::clone(&value)));
        value
    }
}

impl<T> std::fmt::Debug for Memo<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Memo")
    }
}

impl<T> PartialEq for Memo<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

pub type GroupKey = (u8, i64);
/// The group of the reads of the smallest values when a tag has too many values.
const OTHER: GroupKey = (3, 0);

fn group_key(read: &Read, grouping: Grouping, values: &Values) -> GroupKey {
    alignment_group(&read.alignment, grouping, values)
}

/// The group of a read: numbers of a tag in their order, then text values, then reads without
/// the tag.
fn alignment_group(alignment: &Alignment, grouping: Grouping, values: &Values) -> GroupKey {
    match grouping {
        Grouping::None => (0, 0),
        Grouping::Phase => alignment.tags.phase.map_or((1, 0), |phase| (0, phase)),
        Grouping::Strand => (u8::from(alignment.has(REVERSE)), 0),
        Grouping::Tag(name) => match alignment.tags.get(name) {
            None => (2, 0),
            Some(text) => text
                .parse::<i64>()
                .map_or_else(|_| (1, values.number(text)), |number| (0, number)),
        },
    }
}

fn group_label(grouping: Grouping, key: (u8, i64), values: &Values) -> String {
    match (grouping, key) {
        (Grouping::Phase, (0, phase)) => format!("Haplotype {phase}"),
        (Grouping::Phase, _) => "No haplotype".to_owned(),
        (Grouping::Strand, (0, _)) => "Forward strand".to_owned(),
        (Grouping::Strand, _) => "Reverse strand".to_owned(),
        (Grouping::Tag(name), (0, number)) => format!("{} {number}", tag_name(name)),
        (Grouping::Tag(name), (1, number)) => {
            format!("{} {}", tag_name(name), values.name(number))
        }
        (Grouping::Tag(name), (3, _)) => format!("Other {} values", tag_name(name)),
        (Grouping::Tag(name), _) => format!("No {}", tag_name(name)),
        (Grouping::None, _) => String::new(),
    }
}

/// Puts the reads of all but the largest groups of a tag in one group.
fn fewer(keys: &mut [GroupKey]) {
    let mut sizes: HashMap<GroupKey, usize> = HashMap::new();
    for key in keys.iter() {
        *sizes.entry(*key).or_default() += 1;
    }
    if sizes.len() <= TAG_GROUPS {
        return;
    }
    let mut largest: Vec<(GroupKey, usize)> = sizes.into_iter().collect();
    largest.sort_by(|(a, first), (b, second)| second.cmp(first).then(a.cmp(b)));
    let kept: HashSet<GroupKey> = largest
        .iter()
        .take(TAG_GROUPS - 1)
        .map(|(key, _)| *key)
        .collect();
    for key in keys.iter_mut() {
        if !kept.contains(key) {
            *key = OTHER;
        }
    }
}

/// The rows of some reads: each read in the first free row, or with `mates` the two mates of
/// each pair in the first row that is free from the start of one mate to the end of the other.
fn layout(reads: &[Read], chosen: &[usize], mates: bool) -> Vec<usize> {
    let extent = |&index: &usize| reads[index].extent;
    if !mates {
        return rows::pack(chosen, extent, READ_GAP);
    }
    let mut units: Vec<(Region, Vec<usize>)> = Vec::new();
    let mut open: HashMap<&str, usize> = HashMap::new();
    for (place, &index) in chosen.iter().enumerate() {
        let read = &reads[index];
        let alignment = &read.alignment;
        let primary =
            alignment.has(PAIRED) && !alignment.has(SECONDARY) && !alignment.has(SUPPLEMENTARY);
        if primary {
            if let Some(unit) = open.remove(alignment.name.as_str()) {
                units[unit].0 = units[unit].0.hull(read.extent);
                units[unit].1.push(place);
                continue;
            }
            open.insert(&alignment.name, units.len());
        }
        units.push((read.extent, vec![place]));
    }
    let packed = rows::pack(&units, |(span, _)| *span, READ_GAP);
    let mut rows = vec![0; chosen.len()];
    for ((_, places), row) in units.iter().zip(packed) {
        for &place in places {
            rows[place] = row;
        }
    }
    rows
}

fn pack(
    reads: &[Read],
    members: &[usize],
    (grouping, values, mates): (Grouping, &Values, bool),
) -> (Vec<usize>, Vec<(usize, String)>) {
    if grouping == Grouping::None {
        return (layout(reads, members, mates), Vec::new());
    }
    let mut keys: Vec<(u8, i64)> = members
        .iter()
        .map(|&index| group_key(&reads[index], grouping, values))
        .collect();
    let tagged = matches!(grouping, Grouping::Tag(_));
    if tagged {
        fewer(&mut keys);
    }
    let mut order = keys.clone();
    order.sort_unstable();
    order.dedup();
    if tagged {
        order.sort_by(|a, b| {
            a.0.cmp(&b.0).then_with(|| match a.0 {
                1 => values.name(a.1).cmp(values.name(b.1)),
                _ => a.1.cmp(&b.1),
            })
        });
    }
    let mut rows = vec![0; members.len()];
    let mut groups = Vec::new();
    let mut next = 0;
    for key in order {
        let places: Vec<usize> = (0..members.len())
            .filter(|&place| keys[place] == key)
            .collect();
        let chosen: Vec<usize> = places.iter().map(|&place| members[place]).collect();
        let stacked = layout(reads, &chosen, mates);
        for (&place, &row) in places.iter().zip(&stacked) {
            rows[place] = next + row;
        }
        groups.push((next, group_label(grouping, key, values)));
        next += count(&stacked);
    }
    (rows, groups)
}

/// The hue of each read by its value of a tag: the most common values get the hues of the
/// tracks, the other reads get none.
fn tag_hues(reads: &[Read], name: [u8; 2]) -> Vec<Option<Hue>> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for read in reads {
        if let Some(value) = read.alignment.tags.get(name) {
            *counts.entry(value).or_default() += 1;
        }
    }
    let mut common: Vec<(&str, usize)> = counts.into_iter().collect();
    common.sort_by(|(a, first), (b, second)| second.cmp(first).then(a.cmp(b)));
    let chosen: Vec<&str> = common
        .iter()
        .take(Hue::ALL.len())
        .map(|(value, _)| *value)
        .collect();
    reads
        .iter()
        .map(|read| {
            let value = read.alignment.tags.get(name)?;
            let place = chosen.iter().position(|known| *known == value)?;
            Some(Hue::ALL[place])
        })
        .collect()
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
        let (packed, groups) = pack(
            &self.reads,
            &shown,
            (self.grouping, &self.values, self.mates),
        );
        let mut rows = vec![0; self.reads.len()];
        for (&index, &row) in shown.iter().zip(&packed) {
            rows[index] = row;
        }
        (rows, count(&packed), groups)
    }

    pub fn starts(&self) -> Vec<usize> {
        self.groups.iter().map(|(first, _)| *first).collect()
    }

    pub fn arc_height(&self) -> f32 {
        if self.junctions.is_empty() {
            0.0
        } else {
            alignments::JUNCTION_HEIGHT
        }
    }

    pub fn base(&self, position: u64) -> Option<u8> {
        let index = usize::try_from(position.checked_sub(self.offset)?).ok()?;
        self.reference.get(index).copied()
    }
}

#[derive(Debug, PartialEq)]
pub enum Content {
    Features(Annotation),
    Reads(Box<Pile>),
    Variants(Calls),
    Signal(Signal),
    Links(Vec<Interaction>),
    Segments(Copies),
    Contacts(Contacts),
    Associations(Vec<Association>),
    Junctions(Splices),
}

/// The splice junctions of a junction file in a region, and the fields of each for its card.
#[derive(Debug, PartialEq)]
pub struct Splices {
    pub junctions: Vec<Junction>,
    pub fields: Vec<Vec<Field>>,
}

/// The contacts of a Hi-C map, the step that chose the size of its bins, and the normalization
/// of its values.
#[derive(Debug, PartialEq)]
pub struct Contacts {
    pub map: ContactMap,
    pub step: u64,
    pub normalization: String,
}

/// The copy number segments of a region, and the number of samples of the file.
#[derive(Debug, PartialEq)]
pub struct Copies {
    pub segments: Vec<Segment>,
    pub samples: usize,
}

#[derive(Debug)]
pub struct Gather {
    contig: usize,
    options: Options,
    coverage: Coverage,
    inserts: Inserts,
    junctions: Junctions,
    chains: Chains,
    methylation: Tally<GroupKey>,
    sampler: Sampler,
    values: Values,
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
            junctions: Junctions::default(),
            chains: Chains::default(),
            methylation: Tally::default(),
            sampler: Sampler::new(WINDOW, limit),
            values: Values::default(),
        }
    }

    pub fn add(&mut self, alignment: Alignment) {
        self.coverage.add(&alignment);
        self.inserts.add(&alignment);
        self.junctions.add(&alignment);
        self.chains.add(&alignment);
        if let Grouping::Tag(name) = self.options.group
            && let Some(text) = alignment.tags.get(name)
            && text.parse::<i64>().is_err()
        {
            self.values.add(text);
        }
        let group = alignment_group(&alignment, self.options.group, &self.values);
        self.methylation.add(&alignment, group);
        self.sampler.push(alignment);
    }
}

#[derive(Debug, PartialEq)]
pub struct Loaded {
    pub region: Region,
    pub options: Options,
    pub content: Content,
    /// The B-allele frequencies of the sample of `options.baf` in the region, or nothing when
    /// their data is too large to read.
    pub fractions: Option<Arc<[Fraction]>>,
}

impl Loaded {
    pub fn features(region: Region, features: Vec<Feature>) -> Self {
        let rows = rows::pack(&features, Feature::span, 0);
        let count = count(&rows);
        Self {
            region,
            options: Options::default(),
            fractions: None,
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
            junctions,
            chains,
            methylation,
            sampler,
            values,
        } = gather;
        let methylation = methylation.finish(|position| {
            let index = usize::try_from(position.checked_sub(offset)?).ok()?;
            reference.get(index).copied()
        });
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
        let (rows, groups) = pack(&reads, &members, (options.group, &values, options.mates));
        let count = count(&rows);
        let hues = options
            .color
            .map_or_else(Vec::new, |name| tag_hues(&reads, name));
        let junctions = junctions
            .finish()
            .into_iter()
            .map(|mut junction| {
                if junction.strand == Strand::Unknown {
                    junction.strand = motif_strand(&reference, offset, junction.span);
                }
                junction
            })
            .collect();
        Self {
            region,
            options,
            fractions: None,
            content: Content::Reads(Box::new(Pile {
                coverage,
                reference,
                offset,
                reads,
                junctions,
                chains: chains.finish(),
                isoforms: Memo::default(),
                methylation,
                rows,
                count,
                groups,
                grouping: options.group,
                mates: options.mates,
                values,
                hues,
            })),
        }
    }

    pub fn variants(region: Region, variants: Vec<Variant>, samples: usize) -> Self {
        Self {
            region,
            options: Options::default(),
            fractions: None,
            content: Content::Variants(Calls { variants, samples }),
        }
    }

    pub fn links(region: Region, links: Vec<Interaction>) -> Self {
        Self {
            region,
            options: Options::default(),
            fractions: None,
            content: Content::Links(links),
        }
    }

    pub fn junctions(region: Region, splices: Vec<genie_formats::junctions::Splice>) -> Self {
        let (junctions, fields) = splices
            .into_iter()
            .map(|splice| (splice.junction, splice.fields))
            .unzip();
        Self {
            region,
            options: Options::default(),
            fractions: None,
            content: Content::Junctions(Splices { junctions, fields }),
        }
    }

    pub fn associations(region: Region, associations: Vec<Association>) -> Self {
        Self {
            region,
            options: Options::default(),
            fractions: None,
            content: Content::Associations(associations),
        }
    }

    pub fn contacts(region: Region, contacts: Contacts) -> Self {
        Self {
            region,
            options: Options::default(),
            fractions: None,
            content: Content::Contacts(contacts),
        }
    }

    pub fn segments(region: Region, segments: Vec<Segment>, samples: usize) -> Self {
        Self {
            region,
            options: Options::default(),
            fractions: None,
            content: Content::Segments(Copies { segments, samples }),
        }
    }

    pub fn signal(region: Region, summaries: Vec<Summary>, step: u64) -> Self {
        Self {
            region,
            options: Options::default(),
            fractions: None,
            content: Content::Signal(Signal {
                summaries,
                step,
                sites: Vec::new(),
                kept: None,
            }),
        }
    }

    pub fn kept_density(region: Region, kept: Kept, step: u64) -> Self {
        let summaries = signal_reduce(&bbi::depths(kept.kept.iter().copied(), region), step);
        Self {
            region,
            options: Options::default(),
            fractions: None,
            content: Content::Signal(Signal {
                summaries,
                step,
                sites: Vec::new(),
                kept: Some(kept),
            }),
        }
    }

    /// The sites of a bedMethyl file with their percents in bins of `step` bases. The sites stay
    /// for the methylation difference of groups.
    pub fn methylation(region: Region, sites: Vec<Site>, step: u64) -> Self {
        let summaries = signal_reduce(&bedmethyl::percents(&sites), step);
        Self {
            region,
            options: Options::default(),
            fractions: None,
            content: Content::Signal(Signal {
                summaries,
                step,
                sites,
                kept: None,
            }),
        }
    }

    pub fn count(&self) -> usize {
        match &self.content {
            Content::Features(annotation) => annotation.count,
            Content::Reads(pile) => pile.count,
            Content::Variants(calls) => calls.samples,
            Content::Segments(copies) => copies.samples,
            Content::Signal(_)
            | Content::Links(_)
            | Content::Contacts(_)
            | Content::Associations(_)
            | Content::Junctions(_) => 0,
        }
    }

    fn step(&self) -> u64 {
        match &self.content {
            Content::Signal(signal) => signal.step,
            Content::Contacts(contacts) => contacts.step,
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
    /// A hidden track does not show and does not load its data.
    pub hidden: bool,
    /// The folder of the track in the tracks panel. The tracks of a folder follow each other.
    pub folder: Option<String>,
    pub contigs: Vec<Option<String>>,
    pub names: Option<Arc<Names>>,
    pub hue: Hue,
    pub mode: Option<Mode>,
    pub options: Options,
    pub plot: Plot,
    pub height: Option<f32>,
    pub filter: Filter,
    pub modification: Option<String>,
    /// The file type and index that a track hub gave, kept so that sessions open it again.
    pub declared: Option<Declared>,
    /// The fields of the sample table that group, sort and color the samples.
    pub choice: Choice,
    /// The order, the groups and the colors of the samples of the file.
    pub arrangement: Arc<Arrangement>,
    /// The rows of the samples of the file, for genotypes and copy number segments.
    pub rows: Arc<Rows>,
    /// The children of the file whose parents are in the file too, from the sample table.
    pub trios: Arc<[Trio]>,
    /// The chain file that moves the data of the file to the assembly in view.
    pub lift: Option<Arc<Lift>>,
    /// The chain file of a session that genie reads for the track.
    pub lifting: Option<Location>,
    /// The review marks of the variants of the file.
    pub reviews: Arc<Reviews>,
    pub slot: Slot,
    pub extra: Vec<Slot>,
}

/// The file type and index that a track hub gives for a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub kind: String,
    pub index: Option<Location>,
}

/// The number of samples that share a row of a track: one for copy number segments, and more
/// for the genotypes of a big cohort.
pub fn row_size(source: &Source) -> usize {
    if source.has_variants() {
        genie_render::variants::group(source.samples().len())
    } else {
        1
    }
}

impl Track {
    pub fn new(id: u64, source: Arc<Source>, assembly: &Assembly) -> Self {
        let contigs = contig_names(source.contigs(), assembly);
        let range = source.is_bedmethyl().then_some(Scale {
            min: 0.0,
            max: 100.0,
        });
        let arrangement = Arrangement::plain(source.samples().len());
        let rows = Arc::new(Rows::new(&arrangement, row_size(&source)));
        Self {
            id,
            name: source.name.clone(),
            over: None,
            hidden: false,
            folder: None,
            source,
            contigs,
            names: None,
            hue: Hue::default(),
            mode: None,
            options: Options::default(),
            plot: Plot {
                range,
                ..Plot::default()
            },
            height: None,
            filter: Filter::default(),
            modification: None,
            declared: None,
            choice: Choice::default(),
            arrangement: Arc::new(arrangement),
            rows,
            trios: Arc::new([]),
            lift: None,
            lifting: None,
            reviews: Arc::default(),
            slot: Slot::default(),
            extra: Vec::new(),
        }
    }

    /// The key of a variant of a contig in view in the review marks: the name of the contig in
    /// the file, the start and the alleles.
    pub fn review_key(&self, contig: usize, variant: &Variant) -> Option<Key> {
        let name = self.contigs.get(contig)?.as_deref()?;
        Some(Key::of(name, variant))
    }

    /// The review mark of a variant of a contig in view.
    pub fn review_of(&self, contig: usize, variant: &Variant) -> Option<&Review> {
        if self.reviews.is_empty() {
            return None;
        }
        let name = self.contigs.get(contig)?.as_deref()?;
        self.reviews.of(name, variant)
    }

    /// Moves the data of the track to the assembly in view with a lift, or shows the data as it
    /// is without one. Data and names of the earlier lift do not show again.
    pub fn set_lift(&mut self, lift: Option<Arc<Lift>>, assembly: &Assembly) {
        self.contigs = match &lift {
            Some(lift) => contig_names(&lift.contigs(), assembly),
            None => contig_names(self.source.contigs(), assembly),
        };
        self.lift = lift;
        self.lifting = None;
        self.names = None;
        self.options.lift = self.options.lift.wrapping_add(1);
        self.forget();
    }

    /// Shows nothing until genie reads the chain file that a session gives the track. False when
    /// genie already reads that file for the track.
    pub fn wait_for_lift(&mut self, location: &Location) -> bool {
        if self.lifting.as_ref() == Some(location) {
            return false;
        }
        self.contigs.fill(None);
        self.lifting = Some(location.clone());
        true
    }

    /// Puts the samples in the order, the groups and the colors that the fields of `choice`
    /// give them in a sample table, or in the order of the file without a table. The parents in
    /// the table give the trios of a variant track. When the trios change, the filter applies
    /// again, because the inheritance of the variants changes with them.
    pub fn arrange(&mut self, table: Option<&Table>) {
        let names = self.source.samples();
        let arrangement = match table {
            Some(table) if !self.choice.is_empty() => {
                genie_core::samples::arrange(names, table, &self.choice)
            }
            _ => Arrangement::plain(names.len()),
        };
        self.rows = Arc::new(Rows::new(&arrangement, row_size(&self.source)));
        self.arrangement = Arc::new(arrangement);
        let trios: Vec<Trio> = match table {
            Some(table) if self.source.has_variants() => trio::trios(names, table),
            _ => Vec::new(),
        };
        if *self.trios != *trios {
            self.trios = trios.into();
            self.refilter();
        }
    }

    pub fn modification_names(&self) -> Vec<String> {
        if self.source.is_bedmethyl() {
            return self.source.modifications().to_vec();
        }
        match self.slot.loaded.as_ref().map(|(_, data)| &data.content) {
            Some(Content::Reads(pile)) => pile.methylation.names().to_vec(),
            _ => Vec::new(),
        }
    }

    pub fn shown_modification(&self) -> Option<String> {
        if self.source.is_bedmethyl() {
            let codes = self.source.modifications();
            return codes
                .get(usize::from(self.options.methyl.code))
                .or_else(|| codes.first())
                .cloned();
        }
        match self.slot.loaded.as_ref().map(|(_, data)| &data.content) {
            Some(Content::Reads(pile)) => self.methylation_name(pile),
            _ => None,
        }
    }

    /// The name of the normalization that somebody chose for a Hi-C map.
    pub fn normalization(&self) -> Option<String> {
        let place = self.options.contacts.normalization?;
        let (names, _) = self.source.normalizations(Some(place));
        names.get(usize::from(place)).cloned()
    }

    /// Chooses a normalization of a Hi-C map by its name, as a session keeps it.
    pub fn set_normalization(&mut self, name: Option<&str>) {
        let (names, _) = self.source.normalizations(None);
        self.options.contacts.normalization = name
            .and_then(|name| names.iter().position(|known| known == name))
            .and_then(|place| u8::try_from(place).ok());
    }

    pub fn set_modification(&mut self, name: Option<String>) {
        let codes = self.source.modifications();
        let code = name
            .as_ref()
            .and_then(|name| codes.iter().position(|code| code == name))
            .and_then(|code| u8::try_from(code).ok());
        if let Some(code) = code {
            self.options.methyl.code = code;
        }
        self.modification = name;
    }

    pub fn methylation_name(&self, pile: &Pile) -> Option<String> {
        let names = pile.methylation.names();
        self.modification
            .clone()
            .filter(|name| names.contains(name))
            .or_else(|| pile.methylation.busiest().map(str::to_owned))
    }

    /// True when the track shows a methylation difference: of the groups of its reads, or of
    /// its bedMethyl group and the group of a track that it compares with.
    pub fn shows_difference(&self) -> bool {
        let compares = self
            .plot
            .math
            .is_some_and(|math| math.operation == genie_render::signal::Operation::Methylation);
        (self.source.has_reads() && self.plot.look.difference) || compares
    }

    /// The methylation difference of the first two groups of reads in view, when the track shows
    /// it: the names of the two groups, and the difference at each site.
    pub fn methylation_difference(
        &self,
        pile: &Pile,
    ) -> Option<((String, String), Vec<genie_core::methylation::Difference>)> {
        if !self.plot.look.difference {
            return None;
        }
        let (_, lanes) = self.methylation_lanes(pile);
        let [(Some(first), a), (Some(second), b), ..] = lanes.as_slice() else {
            return None;
        };
        let differences = genie_core::methylation::difference(a, b);
        Some(((first.clone(), second.clone()), differences))
    }

    pub fn methylation_lanes(&self, pile: &Pile) -> (Vec<GroupKey>, Vec<MethylationLane>) {
        let name = self.methylation_name(pile).filter(|_| self.plot.look.mods);
        let Some(name) = name else {
            return (Vec::new(), Vec::new());
        };
        pile.methylation
            .groups()
            .into_iter()
            .map(|key| {
                let label = (pile.grouping != Grouping::None)
                    .then(|| group_label(pile.grouping, key, &pile.values));
                (key, (label, pile.methylation.sites(&name, key)))
            })
            .filter(|(_, (_, sites))| !sites.is_empty())
            .unzip()
    }

    pub fn refilter(&mut self) {
        let shown = self
            .slot
            .loaded
            .as_ref()
            .and_then(|(contig, data)| self.derive(*contig, data));
        self.slot.shown = shown;
        for index in 0..self.extra.len() {
            let shown = self.extra[index]
                .loaded
                .as_ref()
                .and_then(|(contig, data)| self.derive(*contig, data));
            self.extra[index].shown = shown;
        }
    }

    /// The trios that check the variants of a contig in view: see `trio::on`.
    pub fn trios_on(&self, contig: usize) -> Vec<Trio> {
        if self.trios.is_empty() {
            return Vec::new();
        }
        let name = self.contigs.get(contig).and_then(Option::as_deref);
        trio::on(name.unwrap_or_default(), &self.trios)
    }

    /// Reads and a zoomed-out bigBed density depend on the filter rules, so a rule change drops
    /// them to load them again.
    pub fn forget_kept(&mut self) {
        let reads = self.source.has_reads();
        let bigbed = matches!(
            self.source.format,
            Format::Features(FeatureFile::BigBed { .. })
        );
        if !reads && !bigbed {
            return;
        }
        for slot in std::iter::once(&mut self.slot).chain(self.extra.iter_mut()) {
            let kept = reads
                || slot
                    .loaded
                    .as_ref()
                    .is_some_and(|(_, data)| matches!(data.content, Content::Signal(_)));
            if kept {
                (slot.loaded, slot.shown, slot.wanted) = (None, None, None);
            }
        }
    }

    fn derive(&self, contig: usize, data: &Loaded) -> Option<Arc<Loaded>> {
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
                let (rules, trios) = (&self.filter.rules, &self.trios_on(contig));
                let variants = calls
                    .variants
                    .iter()
                    .filter(|variant| {
                        let review = self.review_of(contig, variant);
                        let fields = variant_fields(&self.source, variant, trios, review);
                        fields::passes(rules, &fields)
                    })
                    .cloned()
                    .collect();
                Content::Variants(Calls {
                    variants,
                    samples: calls.samples,
                })
            }
            Content::Links(links) => {
                let rules = &self.filter.rules;
                Content::Links(
                    links
                        .iter()
                        .filter(|link| fields::passes(rules, &link_fields(&self.source, link)))
                        .cloned()
                        .collect(),
                )
            }
            Content::Junctions(splices) => {
                let rules = &self.filter.rules;
                let kept: Vec<usize> = (0..splices.junctions.len())
                    .filter(|&at| fields::passes(rules, &splices.fields[at]))
                    .collect();
                Content::Junctions(Splices {
                    junctions: kept.iter().map(|&at| splices.junctions[at]).collect(),
                    fields: kept.iter().map(|&at| splices.fields[at].clone()).collect(),
                })
            }
            Content::Reads(_)
            | Content::Signal(_)
            | Content::Segments(_)
            | Content::Contacts(_)
            | Content::Associations(_) => return None,
        };
        Some(Arc::new(Loaded {
            region: data.region,
            options: data.options,
            content,
            fractions: data.fractions.clone(),
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

    /// Shows the track, or hides it and drops its data.
    pub fn set_visible(&mut self, visible: bool) {
        self.hidden = !visible;
        if self.hidden {
            self.drop_data();
        }
    }

    /// Drops the data of every panel, so that a hidden track costs no memory.
    pub fn drop_data(&mut self) {
        for slot in std::iter::once(&mut self.slot).chain(self.extra.iter_mut()) {
            (slot.loaded, slot.shown, slot.wanted) = (None, None, None);
            (slot.error, slot.order) = (None, None);
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
            } else if self.source.has_segments() {
                genie_render::segments::fit(count)
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
            !self.source.has_density()
                || matches!(data.content, Content::Features(_) | Content::Variants(_))
                    == self.source.detailed(visible)
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
        let shown = result
            .as_ref()
            .ok()
            .and_then(|data| self.derive(contig, data));
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
    Mods,
    /// Show the reads of a track as a Sashimi plot, or show the reads again.
    Sashimi,
    /// Show the isoforms of the reads of a track, or show the reads again.
    Isoforms,
    /// The fewest reads of a splice junction that a Sashimi plot shows.
    JunctionReads(u32),
    Modification(usize),
    Colors,
    Sort(usize, u64),
    Group(Grouping),
    /// The lowest MAPQ of the reads that show.
    Mapq(u8),
    /// Show the B-allele frequency of a sample of a variant track in a lane, or remove the lane.
    Baf(Option<usize>),
    /// Color the reads by the values of a tag, or stop when the tag already colors them.
    Color([u8; 2]),
    /// Normalize a Hi-C map with the normalization at this place in the list of the file.
    Normalization(u8),
    /// Show ratios of observed to expected contacts instead of counts, or go back.
    Ratio,
    /// Show a column of numbers of a table, or its rows.
    Column(Pick),
    Up,
    Down,
    Remove,
    Overlay(u64),
    Separate,
    /// Show the values of a signal track against the values of another signal track, or stop.
    Compare(Option<u64>),
    /// The operation of the comparison of a signal track.
    Operation(Operation),
    /// The viewpoint of the virtual 4C of a Hi-C track, or none to stop it.
    Viewpoint(Option<u64>),
    /// Show the track, or hide it: a hidden track does not show and drops its data.
    Visible(bool),
    /// Show the methylation difference of the first two groups of reads, or stop.
    Difference,
}

pub fn change(tracks: &mut Vec<Track>, id: u64, change: Change) {
    let Some(position) = tracks.iter().position(|track| track.id == id) else {
        return;
    };
    match change {
        Change::Mode(mode) => tracks[position].mode = Some(mode),
        Change::Hue(hue) => tracks[position].hue = hue,
        Change::Visible(visible) => tracks[position].set_visible(visible),
        Change::Difference => tracks[position].plot.look.difference ^= true,
        Change::Style(style) => tracks[position].plot.style = style,
        Change::Link => {
            let plot = &mut tracks[position].plot;
            plot.linked = !plot.linked;
            if plot.linked {
                plot.range = None;
            }
        }
        Change::Pairs => {
            let look = &mut tracks[position].plot.look;
            look.pairs = !look.pairs;
        }
        Change::Mods => {
            let look = &mut tracks[position].plot.look;
            look.mods = !look.mods;
        }
        Change::Sashimi => {
            let plot = &mut tracks[position].plot;
            plot.sashimi = !plot.sashimi;
            plot.isoforms &= !plot.sashimi;
        }
        Change::Isoforms => {
            let plot = &mut tracks[position].plot;
            plot.isoforms = !plot.isoforms;
            plot.sashimi &= !plot.isoforms;
        }
        Change::JunctionReads(reads) => tracks[position].plot.junction_reads = reads.max(1),
        Change::Modification(index) => {
            let track = &mut tracks[position];
            if let Some(name) = track.modification_names().get(index).cloned() {
                track.set_modification(Some(name));
                track.forget();
            }
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
        Change::Mapq(_)
        | Change::Baf(_)
        | Change::Color(_)
        | Change::Normalization(_)
        | Change::Ratio
        | Change::Column(_) => {
            let track = &mut tracks[position];
            track.options = track.options.changed(change);
            track.forget();
        }
        Change::Up if position > 0 => tracks.swap(position, position - 1),
        Change::Down if position + 1 < tracks.len() => tracks.swap(position, position + 1),
        Change::Up | Change::Down => {}
        Change::Remove => {
            tracks.remove(position);
            removed(tracks, id);
        }
        Change::Overlay(base) => overlay(tracks, position, base),
        Change::Separate => {
            if tracks[position].over.take().is_none() {
                free(tracks, id);
            }
        }
        Change::Compare(other) => compare(tracks, position, other),
        Change::Operation(operation) => operate(&mut tracks[position], operation),
        Change::Viewpoint(viewpoint) => tracks[position].plot.viewpoint = viewpoint,
    }
}

/// Puts a track in a folder after the other tracks of the folder, or takes it out of its folder
/// after the tracks that stay in it, so that the tracks of each folder follow each other.
pub fn file(tracks: &mut Vec<Track>, position: usize, folder: Option<String>) {
    let mut track = tracks.remove(position);
    let block = folder.as_ref().or(track.folder.as_ref());
    let last = block.and_then(|name| {
        tracks
            .iter()
            .rposition(|other| other.folder.as_ref() == Some(name))
    });
    track.folder = folder.filter(|name| !name.trim().is_empty());
    let at = last.map_or(position.min(tracks.len()), |last| last + 1);
    tracks.insert(at, track);
}

/// Frees the tracks that overlay a removed track, and stops the comparisons with it.
fn removed(tracks: &mut [Track], id: u64) {
    free(tracks, id);
    for track in tracks
        .iter_mut()
        .filter(|track| track.plot.math.is_some_and(|math| math.other == id))
    {
        track.plot.math = None;
    }
}

/// Changes the operation of the comparison of a signal track.
fn operate(track: &mut Track, operation: Operation) {
    if let Some(math) = &mut track.plot.math {
        math.operation = operation;
    }
}

/// True for a track that is not in an overlay: it overlays no track, and no track overlays it.
fn alone(tracks: &[Track], track: &Track) -> bool {
    track.over.is_none() && !tracks.iter().any(|other| other.over == Some(track.id))
}

/// Compares a signal track with another signal track, or stops: its lane shows the log2 ratio of
/// their values until another operation is chosen. A track in an overlay cannot compare.
fn compare(tracks: &mut [Track], position: usize, other: Option<u64>) {
    let Some(other) = other else {
        tracks[position].plot.math = None;
        return;
    };
    let track = &tracks[position];
    let fits = track.source.has_signal()
        && alone(tracks, track)
        && other != track.id
        && tracks
            .iter()
            .any(|known| known.id == other && known.source.has_signal());
    if fits {
        tracks[position].plot.math = Some(Math {
            other,
            operation: Operation::Log2Ratio,
        });
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
        && tracks[position].plot.math.is_none()
        && !tracks.iter().any(|track| track.over == Some(id))
        && tracks.iter().any(|track| {
            track.id == base
                && track.id != id
                && track.over.is_none()
                && track.plot.math.is_none()
                && track.source.has_signal()
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
        || is_cram(&name)
        || is_vcf(&name)
        || is_bcf(&name)
        || is_signal(&name)
        || is_bedmethyl(&name)
        || is_bigbed(&name)
        || is_bedpe(&name)
        || is_segments(&name)
        || is_hic(&name)
        || is_gwas_name(&name)
        || genie_formats::junctions::Layout::of(&name).is_some()
        || genie_formats::genbank::is_name(&name)
        || Kind::of(&name).is_some()
}

pub fn is_bedpe(name: &str) -> bool {
    inner_extension(name, &["bedpe"])
}

/// True for the names of the GWAS files of PLINK 2 (`.glm.linear`, `.glm.logistic`, …) and
/// REGENIE (`.regenie`), and for `.sumstats` files, also compressed.
pub fn is_gwas_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower
        .strip_suffix(".gz")
        .or_else(|| lower.strip_suffix(".bgz"))
        .unwrap_or(&lower);
    stem.contains(".glm.") || stem.ends_with(".regenie") || stem.ends_with(".sumstats")
}

/// True for GWAS summary statistics: by the name of the file, or by a header line that names
/// the chromosome, the position and the p-value. For a file on the web, this reads its first
/// lines.
pub fn is_gwas(location: &Location, name: &str) -> bool {
    if is_gwas_name(name) {
        return true;
    }
    Kind::of(name).is_none()
        && head(location, 20)
            .iter()
            .find(|line| !line.trim().is_empty() && !line.starts_with("##"))
            .is_some_and(|line| genie_formats::gwas::Layout::sniff(line).is_some())
}

/// True when a tabix index (`.tbi` or `.csi`) is next to the file. For a file on the web,
/// this asks the server.
pub fn has_index(location: &Location) -> bool {
    [".tbi", ".csi"]
        .into_iter()
        .any(|suffix| location.with_suffix(suffix).open().is_ok())
}

/// True for Hi-C files: the `.hic` files of Juicer, and cooler files (`.cool`, `.mcool`).
pub fn is_hic(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("hic"))
        || genie_formats::cool::is_cooler(name)
}

/// True for copy number segment files: SEG, and `CNVkit` `.cns` and `.cnr`.
pub fn is_segments(name: &str) -> bool {
    inner_extension(name, &["seg", "cns", "cnr"])
}

pub fn is_bam(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bam"))
}

pub fn is_cram(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("cram"))
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

pub fn is_vcf(name: &str) -> bool {
    inner_extension(name, &["vcf"])
}

/// True for BCF, the binary form of VCF.
pub fn is_bcf(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bcf"))
}

pub fn is_bigwig(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("bw") || extension.eq_ignore_ascii_case("bigwig")
    })
}

pub fn is_bigbed(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("bb") || extension.eq_ignore_ascii_case("bigbed")
    })
}

pub fn is_bedmethyl(name: &str) -> bool {
    inner_extension(name, &["bedmethyl"])
}

fn is_bed(name: &str) -> bool {
    Kind::of(name) == Some(Kind::Bed)
}

/// The first lines of a text file, plain or compressed, at most `count`. A file that genie
/// cannot read has none.
pub fn head(location: &Location, count: usize) -> Vec<String> {
    let Ok(file) = location.open() else {
        return Vec::new();
    };
    let reader: Box<dyn BufRead> = if is_compressed(&location.name()) {
        Box::new(BufReader::new(gzip::Reader::new(BufReader::new(file))))
    } else {
        Box::new(BufReader::new(file))
    };
    reader.lines().take(count).map_while(Result::ok).collect()
}

/// True for a loop list in a text file, as Juicer and `HiCCUPS` write it: its first line is a
/// header such as `chr1 x1 x2 chr2 y1 y2 color o`.
pub fn is_loop_list(location: &Location) -> bool {
    inner_extension(&location.name(), &["txt"])
        && head(location, SNIFF_LINES)
            .iter()
            .find(|line| !line.trim().is_empty())
            .is_some_and(|line| genie_formats::bedpe::is_loop_list(line))
}

fn sniffs_methyl(location: &Location) -> bool {
    let Ok(file) = location.open() else {
        return false;
    };
    let reader: Box<dyn BufRead> = if is_compressed(&location.name()) {
        Box::new(BufReader::new(gzip::Reader::new(BufReader::new(file))))
    } else {
        Box::new(BufReader::new(file))
    };
    for line in reader.lines().take(SNIFF_LINES).map_while(Result::ok) {
        match bedmethyl::parse(&line) {
            Ok(None) => {}
            Ok(Some(_)) => return true,
            Err(_) => return false,
        }
    }
    false
}

pub fn is_bedgraph(name: &str) -> bool {
    inner_extension(name, &["bedgraph", "bdg"])
}

fn is_signal(name: &str) -> bool {
    is_bigwig(name) || is_bedgraph(name)
}

pub fn is_compressed(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("gz") || extension.eq_ignore_ascii_case("bgz")
    })
}

fn plain(location: &Location, name: &str) -> Result<BufReader<Reader>, String> {
    fits(name, location.size()?)?;
    location.open().map(BufReader::new)
}

fn tabix_index(
    location: &Location,
    name: &str,
    index: Option<&Location>,
) -> Result<tabix::Index, String> {
    let (file, label) = match index {
        Some(index) => (index.open()?, index.name()),
        None => [".tbi", ".csi"]
            .into_iter()
            .find_map(|suffix| {
                Some((
                    location.with_suffix(suffix).open().ok()?,
                    format!("{name}{suffix}"),
                ))
            })
            .ok_or_else(|| format!("There is no index {name}.tbi. Index the file with tabix."))?,
    };
    tabix::Index::read(file).map_err(|error| format!("The index {label} is not valid. {error}"))
}

pub fn open(location: impl Into<Location>) -> Result<Source, String> {
    open_with(location, None)
}

/// A track of the sites of the picked motifs of a motif file in the reference, at a threshold of
/// p. No picks stand for all motifs of the file.
pub fn motif_sites(
    location: Location,
    set: Arc<genie_formats::motifs::MotifSet>,
    (picks, p): (Vec<usize>, f64),
    reference: Arc<crate::reference::Reference>,
) -> Source {
    let file = MotifFile::new(set, picks, p, reference);
    Source {
        name: file.name(&location.name()),
        location,
        data: None,
        format: Format::Features(FeatureFile::Motifs(file)),
    }
}

/// True for the files of DNA motifs: by their names, or by their first lines for the text files
/// of JASPAR.
pub fn is_motifs(location: &Location) -> bool {
    let name = location.name();
    genie_formats::motifs::is_name(&name)
        || (inner_extension(&name, &["txt"]) && genie_formats::motifs::sniff(&head(location, 4)))
}

/// A track that genie makes from the bases of the reference, with no file.
pub fn make(made: Made, reference: Arc<crate::reference::Reference>) -> Source {
    let (location, name) = (Location::Made(made.key()), made.name());
    let gc = matches!(made, Made::Gc(_));
    let file = MadeFile::new(made, reference);
    let format = if gc {
        Format::Signal(SignalFile::Gc(file))
    } else {
        Format::Features(FeatureFile::Made(file))
    };
    Source {
        location,
        data: None,
        name,
        format,
    }
}

pub fn open_with(
    location: impl Into<Location>,
    prepared: Option<Prepared>,
) -> Result<Source, String> {
    let location = location.into();
    let name = location.name();
    let (data, index) = match prepared {
        Some(Prepared { data, index }) => (Some(data), Some(index)),
        None => (None, None),
    };
    let reading = data.as_ref().unwrap_or(&location);
    let index = index.as_ref();
    let format = if is_bam(&name) || is_cram(&name) {
        Format::Reads(ReadFile::open(reading, &name, index)?)
    } else if genie_formats::genbank::is_name(&name) {
        Format::Features(FeatureFile::records(reading)?)
    } else if is_vcf(&name) || is_bcf(&name) {
        Format::Variants(VariantFile::open(reading, &name, index)?)
    } else if is_bigbed(&name) || is_bigwig(&name) {
        open_bbi(reading, &name, crate::catalog::label(&location.to_string()))?
    } else if is_hic(&name) {
        Format::Contacts(ContactFile::open(reading, &name)?)
    } else if let Some(layout) = junctions::layout(reading, &name) {
        Format::Junctions(JunctionFile::open(reading, &name, layout)?)
    } else if is_bedpe(&name) || is_loop_list(reading) {
        Format::Links(LinkFile::open(reading, &name)?)
    } else if is_segments(&name) {
        Format::Segments(SegmentFile::open(reading, &name)?)
    } else if is_bedmethyl(&name) || (is_bed(&name) && sniffs_methyl(reading)) {
        Format::Signal(SignalFile::open_methyl(reading, &name, index)?)
    } else if is_signal(&name) {
        Format::Signal(SignalFile::open(reading, &name, index)?)
    } else if is_gwas(reading, &name) {
        Format::Gwas(GwasFile::open(reading, &name, index)?)
    } else if Kind::of(&name).is_none()
        && is_compressed(&name)
        && (index.is_some() || has_index(reading))
    {
        Format::Columns(ColumnFile::open(reading, &name, index)?)
    } else {
        Format::Features(FeatureFile::open(reading, &name, index)?)
    };
    Ok(Source {
        location,
        data,
        name,
        format,
    })
}

/// How genie reads a track whose file type a track hub gives, so the file name can be anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typed {
    /// bigBed, bigWig and the other UCSC big formats.
    Big,
    Bam,
    Cram,
    Vcf,
    /// The Hi-C files of Juicer.
    Hic,
}

impl Typed {
    /// The way to read a hub type such as `bigBed`, `bam` or `vcfTabix`, if genie can read it.
    pub fn of(kind: &str) -> Option<Self> {
        match kind {
            "bam" => Some(Self::Bam),
            "cram" => Some(Self::Cram),
            "vcf" | "vcfTabix" | "vcfPhasedTrio" => Some(Self::Vcf),
            "hic" => Some(Self::Hic),
            _ if kind.starts_with("big") => Some(Self::Big),
            _ => None,
        }
    }
}

/// Opens a track with a file type from a track hub, and with the index that the hub names.
pub fn open_typed(
    location: Location,
    typed: Typed,
    index: Option<&Location>,
) -> Result<Source, String> {
    let name = location.name();
    let format = match typed {
        Typed::Big => open_bbi(&location, &name, None)?,
        Typed::Bam => Format::Reads(ReadFile::open_bam(&location, &name, index)?),
        Typed::Cram => Format::Reads(ReadFile::open_cram(&location, &name, index)?),
        Typed::Vcf => Format::Variants(VariantFile::open(&location, &name, index)?),
        Typed::Hic => Format::Contacts(ContactFile::open(&location, &name)?),
    };
    Ok(Source {
        location,
        data: None,
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
    Ok(if index.is_interact() {
        Format::Links(LinkFile::Big { index })
    } else if index.is_bed() {
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
                Kind::Paf => genie_formats::paf::fields(feature.extra()),
                Kind::Psl => genie_formats::psl::fields(feature.extra()),
            }
        }
        Format::Columns(file) => file.fields(feature.extra()),
        Format::Features(FeatureFile::Made(file)) => file.fields(feature.extra()),
        Format::Features(FeatureFile::Motifs(_)) => MotifFile::fields(feature.extra()),
        Format::Features(FeatureFile::Records { .. }) => {
            genie_formats::genbank::fields(feature.extra())
        }
        _ => Vec::new(),
    }
}

/// The fields of a read for its card and its filters: MAPQ, the insert size of a pair, and the
/// small tags.
pub fn read_fields(read: &Alignment) -> Vec<Field> {
    let mut found = vec![Field {
        label: "MAPQ".to_owned(),
        value: read.mapq.to_string(),
    }];
    if let Some(mate) = read
        .mate
        .filter(|mate| read.has(PAIRED) && mate.insert != 0)
    {
        found.push(Field {
            label: "Insert size".to_owned(),
            value: mate.insert.unsigned_abs().to_string(),
        });
    }
    found.extend(read.tags.all.iter().map(|(name, value)| Field {
        label: String::from_utf8_lossy(name).into_owned(),
        value: value.clone(),
    }));
    found
}

pub fn link_fields(source: &Source, link: &Interaction) -> Vec<Field> {
    match &source.format {
        Format::Links(file) => file.fields(link),
        _ => Vec::new(),
    }
}

/// The name of the field of a variant that tells what the calls of the trios of a family say.
pub const INHERITANCE: &str = "Inheritance";
/// The name of the field of the review mark of a variant.
pub const REVIEW: &str = "Review";
/// The name of the field of the review note of a variant.
pub const REVIEW_NOTE: &str = "Review note";

/// The fields of a variant for its card, its filters and the variant table. With the trios of
/// the track, the Inheritance field lists what their calls say: Mendelian error, De novo and
/// Consistent. The review mark of the variant gives the first fields.
pub fn variant_fields(
    source: &Source,
    variant: &Variant,
    trios: &[Trio],
    review: Option<&Review>,
) -> Vec<Field> {
    let Format::Variants(file) = &source.format else {
        return Vec::new();
    };
    let mut found = Vec::new();
    if let Some(review) = review {
        found.push(Field {
            label: REVIEW.to_owned(),
            value: review.verdict.label().to_owned(),
        });
        if !review.note.is_empty() {
            found.push(Field {
                label: REVIEW_NOTE.to_owned(),
                value: review.note.clone(),
            });
        }
    }
    if let Some(quality) = variant.quality {
        found.push(Field {
            label: "Quality".to_owned(),
            value: quality.to_string(),
        });
    }
    if fields::shown(&variant.filter) {
        found.push(Field {
            label: "Filter".to_owned(),
            value: variant.filter.clone(),
        });
    }
    let biallelic = variant.alternates.len() <= 1;
    let inheritance = trio::words(&variant.calls, trios, biallelic);
    if !inheritance.is_empty() {
        found.push(Field {
            label: INHERITANCE.to_owned(),
            value: inheritance.join(", "),
        });
    }
    found.extend(
        variant
            .annotations
            .fields
            .iter()
            .map(|(label, value)| Field {
                label: label.clone(),
                value: value.clone(),
            }),
    );
    found.extend(vcf::fields(&variant.info, file.infos()));
    found
}

#[cfg(test)]
pub fn load(
    source: &Source,
    contig: &str,
    region: Region,
    options: Options,
    step: u64,
    detail: bool,
    reference: impl FnMut(Region) -> Result<Vec<u8>, String>,
) -> Result<Loaded, String> {
    let scope = Scope {
        step,
        detail,
        rules: &[],
        length: region.end(),
    };
    load_kept(source, contig, region, options, scope, reference)
}

/// How to load a region: the step of the summaries, whether to load the records or their
/// density, the filter, and the length of the contig.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub step: u64,
    pub detail: bool,
    pub rules: &'a [Rule],
    pub length: u64,
}

/// Loads like `load`, and a zoomed-out bigBed density counts only the features that pass the
/// rules when their data is at most `DENSITY_BUDGET` bytes.
pub fn load_kept(
    source: &Source,
    contig: &str,
    region: Region,
    options: Options,
    scope: Scope<'_>,
    reference: impl FnMut(Region) -> Result<Vec<u8>, String>,
) -> Result<Loaded, String> {
    let location = source.data();
    let loaded = match &source.format {
        Format::Features(file) => file.load(location, contig, region, scope, DENSITY_BUDGET)?,
        Format::Variants(file) => {
            let loaded = file.load(location, contig, region, scope)?;
            match options.baf {
                Some(sample) => {
                    let found = file.fractions(location, (contig, region), sample)?;
                    Loaded {
                        fractions: found.map(Arc::from),
                        ..loaded
                    }
                }
                None => loaded,
            }
        }
        Format::Reads(file) => {
            file.load(location, contig, region, (options, scope.rules), reference)?
        }
        Format::Signal(file) => file.load(location, contig, region, scope.step, options.methyl)?,
        Format::Links(file) => file.load(location, contig, region)?,
        Format::Segments(file) => file.load(contig, region),
        Format::Contacts(file) => {
            file.load(location, contig, region, (scope.step, options.contacts))?
        }
        Format::Columns(file) => {
            file.load(location, contig, region, (scope.step, options.table))?
        }
        Format::Gwas(file) => file.load(location, contig, region)?,
        Format::Junctions(file) => file.load(contig, region),
    };
    Ok(Loaded { options, ..loaded })
}

/// Loads a region of a contig in view for a track, through its lift when it has one.
pub fn load_track(
    source: &Source,
    lift: Option<&Lift>,
    (contig, region): (&str, Region),
    options: Options,
    scope: Scope<'_>,
    reference: impl FnMut(Region) -> Result<Vec<u8>, String>,
) -> Result<Loaded, String> {
    match lift {
        Some(lift) => load_lifted(source, lift, (contig, region), options, scope),
        None => load_kept(source, contig, region, options, scope, reference),
    }
}

/// The contig in view and the span of a span of a contig of a file, through a lift when the
/// track has one.
fn placed(
    assembly: &Assembly,
    lift: Option<&Lift>,
    contig: &str,
    span: Region,
) -> Option<(usize, Region)> {
    match lift {
        Some(lift) => {
            let (name, span, _) = lift.place(contig, span)?;
            Some((assembly.find(name)?, span))
        }
        None => Some((assembly.find(contig)?, span)),
    }
}

/// The fields of the row of an association for its card.
pub fn association_fields(source: &Source, association: &Association) -> Vec<Field> {
    match (&source.format, &association.row) {
        (Format::Gwas(file), Some(row)) => file.fields(row),
        _ => Vec::new(),
    }
}

/// The rules of the filter of a track, the trios that give its variants their Inheritance
/// field, and the review marks that give their Review fields.
pub type Keep<'a> = (&'a [Rule], &'a [Trio], &'a Reviews);

pub fn neighbor(
    source: &Source,
    names: &[Option<String>],
    assembly: &Assembly,
    contig: usize,
    position: u64,
    forward: bool,
    keep: Keep<'_>,
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
        if let Some(span) = nearest(source, name, length, threshold, forward, keep)? {
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
    keep: Keep<'_>,
) -> Result<Option<Region>, String> {
    let spans = |source: &Source, name: &str, window: Region| kept(source, name, window, keep);
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
        Format::Features(file) => file.spans(source.data(), contig, region),
        Format::Variants(file) => file.spans(source.data(), contig, region),
        Format::Reads(_)
        | Format::Signal(_)
        | Format::Links(_)
        | Format::Segments(_)
        | Format::Contacts(_)
        | Format::Columns(_)
        | Format::Gwas(_)
        | Format::Junctions(_) => Ok(Vec::new()),
    }
}

fn kept(
    source: &Source,
    contig: &str,
    region: Region,
    (rules, trios, reviews): Keep<'_>,
) -> Result<Vec<Region>, String> {
    if rules.is_empty() {
        return spans(source, contig, region);
    }
    let trios = &trio::on(contig, trios);
    Ok(match &source.format {
        Format::Features(file) => file
            .features(source.data(), contig, region)?
            .1
            .iter()
            .filter(|feature| fields::passes(rules, &feature_fields(source, feature)))
            .map(Feature::span)
            .collect(),
        Format::Variants(file) => file
            .variants(source.data(), contig, region)?
            .iter()
            .filter(|variant| {
                let review = reviews.of(contig, variant);
                fields::passes(rules, &variant_fields(source, variant, trios, review))
            })
            .map(|variant| variant.span)
            .collect(),
        Format::Reads(_)
        | Format::Signal(_)
        | Format::Links(_)
        | Format::Segments(_)
        | Format::Contacts(_)
        | Format::Columns(_)
        | Format::Gwas(_)
        | Format::Junctions(_) => Vec::new(),
    })
}

/// True when the track has a feature or a variant with this name at this place and the filter
/// hides all of them.
pub fn hides(source: &Source, contig: &str, span: Region, name: &str, keep: Keep<'_>) -> bool {
    let (rules, trios, reviews) = keep;
    if rules.is_empty() {
        return false;
    }
    let trios = &trio::on(contig, trios);
    let same = |other: &str| other.eq_ignore_ascii_case(name);
    let passes: Vec<bool> = match &source.format {
        Format::Features(file) => file
            .features(source.data(), contig, span)
            .map(|(_, features)| {
                features
                    .iter()
                    .filter(|feature| same(feature.name()) && feature.span() == span)
                    .map(|feature| fields::passes(rules, &feature_fields(source, feature)))
                    .collect()
            })
            .unwrap_or_default(),
        Format::Variants(file) => file
            .variants(source.data(), contig, span)
            .map(|variants| {
                variants
                    .iter()
                    .filter(|variant| same(&variant.id) && variant.span == span)
                    .map(|variant| {
                        let review = reviews.of(contig, variant);
                        fields::passes(rules, &variant_fields(source, variant, trios, review))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Format::Reads(_)
        | Format::Signal(_)
        | Format::Links(_)
        | Format::Segments(_)
        | Format::Contacts(_)
        | Format::Columns(_)
        | Format::Gwas(_)
        | Format::Junctions(_) => Vec::new(),
    };
    !passes.is_empty() && passes.iter().all(|kept| !kept)
}

/// The first feature of a name in the name indexes of bigBed files, with the lift of its track.
pub fn lookup(
    sources: &[(Arc<Source>, Option<Arc<Lift>>)],
    name: &str,
    assembly: &Assembly,
) -> Option<Named> {
    let name = name.trim();
    sources.iter().find_map(|(source, lift)| {
        let Format::Features(file) = &source.format else {
            return None;
        };
        file.find(source.data(), name)
            .into_iter()
            .find_map(|(contig, feature)| {
                let (contig, span) = placed(assembly, lift.as_deref(), &contig, feature.span())?;
                Some(Named {
                    name: feature.name().to_owned(),
                    contig,
                    span,
                })
            })
    })
}

/// The names of the features or variants of a file, on the assembly in view through the lift of
/// the track when it has one.
pub fn names(source: &Source, assembly: &Assembly, lift: Option<&Lift>) -> Result<Names, String> {
    if source.data().is_remote() {
        return Ok(Names::default());
    }
    let entries = match &source.format {
        Format::Features(file) => file.names(source.data())?,
        Format::Variants(file) => file.names(source.data())?,
        Format::Reads(_)
        | Format::Signal(_)
        | Format::Links(_)
        | Format::Segments(_)
        | Format::Contacts(_)
        | Format::Columns(_)
        | Format::Gwas(_)
        | Format::Junctions(_) => {
            return Ok(Names::default());
        }
    };
    Ok(Names::new(entries.into_iter().filter_map(|entry| {
        let (contig, span) = placed(assembly, lift, &entry.contig, entry.span)?;
        Some(Named {
            contig,
            name: entry.name,
            span,
        })
    })))
}

#[cfg(test)]
mod tests {
    use std::fs;
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
    use genie_render::signal::{Scale, Style};

    use super::reads::Reads;
    use super::{
        Change, Contacts, Content, FeatureFile, Format, Gather, Grouping, Kept, Kinds, Loaded,
        Methyl, Options, Pick, Plot, Prepared, Scope, Showing, SignalFile, Source, Splices,
        TAG_GROUPS, Toggle, Track, Typed, VariantFile, change, contig_names, feature_fields, fits,
        hides, is_track, junctions, link_fields, load, load_kept, lookup, names, neighbor, open,
        open_bbi, open_typed, open_with, spans, step, variant_fields,
    };
    use crate::location::Location;
    use crate::scratch::Scratch;
    use genie_formats::fields::{Rule, Test};

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
            data: None,
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
        assert!(tracks[1].plot.look.pairs);
        tracks[1].slot.wanted = Some((0, region(0, 10)));
        change(&mut tracks, 1, Change::Pairs);
        assert!(!tracks[1].plot.look.pairs);
        assert_eq!(tracks[1].slot.wanted, Some((0, region(0, 10))));
    }

    #[test]
    fn changes_toggle_the_modification_colors() {
        let mut tracks = three();
        assert!(!tracks[1].plot.look.mods);
        change(&mut tracks, 1, Change::Mods);
        assert!(tracks[1].plot.look.mods);
        change(&mut tracks, 1, Change::Mods);
        assert!(!tracks[1].plot.look.mods);
    }

    #[test]
    fn grouping_packs_each_group_under_its_label() {
        let phased = |start: u64, phase: Option<i64>| Alignment {
            tags: Tags {
                phase,
                ..Tags::default()
            },
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
    fn read_tracks_turn_into_sashimi_plots() {
        let mut tracks = three();
        assert!(!tracks[1].plot.sashimi);
        assert_eq!(tracks[1].plot.junction_reads, 1);
        change(&mut tracks, 1, Change::Sashimi);
        change(&mut tracks, 1, Change::JunctionReads(5));
        assert!(tracks[1].plot.sashimi);
        assert_eq!(tracks[1].plot.junction_reads, 5);
        change(&mut tracks, 1, Change::Sashimi);
        assert!(!tracks[1].plot.sashimi);
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

    fn tagged(start: u64, tags: &[(&[u8; 2], &str)]) -> Alignment {
        let mut read = alignment(start, vec![(Op::Match, 10)]);
        read.tags.all = tags
            .iter()
            .map(|(name, value)| (**name, (*value).to_owned()))
            .collect();
        read
    }

    #[test]
    fn tag_groups_put_numbers_then_text_then_reads_without_the_tag() {
        let reads = vec![
            tagged(10, &[(b"CB", "TTG")]),
            tagged(10, &[(b"CB", "12")]),
            tagged(10, &[]),
            tagged(10, &[(b"CB", "AAC")]),
            tagged(10, &[(b"CB", "3")]),
        ];
        let options = Options {
            group: Grouping::Tag(*b"CB"),
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
        assert_eq!(labels, ["CB 3", "CB 12", "CB AAC", "CB TTG", "No CB"]);
        let many: Vec<Alignment> = (0..40)
            .map(|index| tagged(10, &[(b"CB", &format!("cell{index:02}"))]))
            .collect();
        let loaded = piled(region(0, 100), many, options);
        let Content::Reads(pile) = &loaded.content else {
            panic!("expected reads");
        };
        assert_eq!(pile.groups.len(), TAG_GROUPS);
        assert_eq!(pile.groups.last().unwrap().1, "Other CB values");
    }

    #[test]
    fn tag_colors_go_to_the_most_common_values() {
        let mut reads: Vec<Alignment> = Vec::new();
        for (value, count) in [("a", 1), ("b", 6), ("c", 5), ("d", 4), ("e", 3), ("f", 2)] {
            reads.extend((0..count).map(|_| tagged(10, &[(b"XT", value)])));
        }
        reads.push(tagged(10, &[]));
        let options = Options {
            color: Some(*b"XT"),
            downsample: false,
            ..Options::default()
        };
        let loaded = piled(region(0, 100), reads, options);
        let Content::Reads(pile) = &loaded.content else {
            panic!("expected reads");
        };
        let hue = |value: &str| {
            pile.reads
                .iter()
                .zip(&pile.hues)
                .find(|(read, _)| read.alignment.tags.get(*b"XT") == Some(value))
                .and_then(|(_, hue)| *hue)
        };
        assert_eq!(hue("b"), Some(Hue::ALL[0]));
        assert_eq!(hue("c"), Some(Hue::ALL[1]));
        assert_eq!(hue("f"), Some(Hue::ALL[4]));
        assert_eq!(hue("a"), None);
        assert_eq!(pile.hues.len(), pile.reads.len());
        let plain = piled(region(0, 100), vec![tagged(10, &[])], Options::default());
        let Content::Reads(pile) = &plain.content else {
            panic!("expected reads");
        };
        assert_eq!(pile.hues.len(), 0);
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

    #[test]
    fn loaded_reads_count_junctions_of_all_reads() {
        let spliced = |start| {
            alignment(
                start,
                vec![(Op::Match, 10), (Op::Skip, 50), (Op::Match, 10)],
            )
        };
        let mut alignments: Vec<Alignment> = (0..150).map(|_| spliced(10)).collect();
        alignments.push(alignment(30, vec![(Op::Match, 10)]));
        let loaded = piled(region(0, 1_000), alignments, Options::default());
        let Content::Reads(pile) = &loaded.content else {
            panic!("expected reads");
        };
        assert!(pile.reads.len() < 150, "{}", pile.reads.len());
        let found: Vec<(Region, u32)> = pile
            .junctions
            .iter()
            .map(|junction| (junction.span, junction.count))
            .collect();
        assert_eq!(found, [(region(20, 70), 150)]);
    }

    #[test]
    fn loaded_reads_tally_methylation_of_all_reads_per_group() {
        use genie_core::methylation::Site;
        use genie_core::modification::{Call, Code, Modifications};
        let methylated = |phase: i64, chance: u8| {
            let mut read = alignment(10, vec![(Op::Match, 10)]);
            read.tags.phase = Some(phase);
            read.tags.mods = Modifications {
                codes: vec![Code {
                    base: b'C',
                    code: "m".to_owned(),
                }],
                calls: vec![Call {
                    at: 2,
                    code: 0,
                    chance,
                }],
            };
            read
        };
        let mut alignments: Vec<Alignment> = (0..150).map(|_| methylated(1, 200)).collect();
        alignments.extend((0..30).map(|_| methylated(2, 10)));
        let options = Options {
            group: Grouping::Phase,
            ..Options::default()
        };
        let loaded = piled(region(0, 1_000), alignments, options);
        let Content::Reads(pile) = &loaded.content else {
            panic!("expected reads");
        };
        assert!(pile.reads.len() < 180, "{}", pile.reads.len());
        let site = |modified, total| Site {
            position: 12,
            modified,
            total,
        };
        assert_eq!(pile.methylation.sites("5mC", (0, 1)), [site(150, 150)]);
        assert_eq!(pile.methylation.sites("5mC", (0, 2)), [site(0, 30)]);
    }

    #[test]
    fn untagged_junctions_take_the_strand_of_their_motif() {
        use genie_core::Strand;
        let wide = region(0, 200);
        let mut reference = vec![b'A'; 200];
        for (at, text) in [(20, b"GT"), (68, b"AG"), (100, b"GT"), (148, b"AG")] {
            reference[at..at + 2].copy_from_slice(text);
        }
        let spliced = |start| {
            alignment(
                start,
                vec![(Op::Match, 10), (Op::Skip, 50), (Op::Match, 10)],
            )
        };
        let mut tagged = spliced(90);
        tagged.tags.xs = Some(Strand::Reverse);
        let mut gather = Gather::new(wide, 0, Options::default());
        gather.add(spliced(10));
        gather.add(tagged);
        let loaded = Loaded::reads(gather, reference, 0);
        let Content::Reads(pile) = &loaded.content else {
            panic!("expected reads");
        };
        let strands: Vec<Strand> = pile
            .junctions
            .iter()
            .map(|junction| junction.strand)
            .collect();
        assert_eq!(strands, [Strand::Forward, Strand::Reverse]);
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
        let names = names(&source, &assembly(), None).unwrap();
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
                data: None,
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

    /// A plain VCF file, then the same file with bgzip and a tabix index: the B-allele
    /// frequencies of a sample load with the variants or with their density.
    #[test]
    fn variant_tracks_load_the_b_allele_frequency_of_a_sample() {
        let scratch = Scratch::new("fractions");
        let text = "##fileformat=VCFv4.2\n\
                    #CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\tFORMAT\tNA1\tNA2\n\
                    chr1\t100\t.\tA\tG\t.\tPASS\t.\tGT:AD\t0/1:12,10\t0/0:30,0\n\
                    chr1\t200\t.\tC\tT\t.\tPASS\t.\tGT:AD\t1/1:0,25\t0/1:4,4\n";
        let picked = |sample| Options {
            baf: Some(sample),
            ..Options::default()
        };
        let places = |loaded: &Loaded| -> Vec<(u64, f32)> {
            let found = loaded.fractions.as_deref().unwrap().iter();
            found.map(|site| (site.position, site.value)).collect()
        };
        let plain = open(scratch.file("calls.vcf", text)).unwrap();
        let near = region(0, 1_000);
        let loaded = load(&plain, "chr1", near, picked(1), 1, true, no_reference).unwrap();
        assert_eq!(places(&loaded), [(99, 0.0)]);
        assert_eq!(loaded.options.baf, Some(1));
        let mut writer = genie_formats::bgzf::Writer::new(Vec::new());
        std::io::Write::write_all(&mut writer, text.as_bytes()).unwrap();
        let data = writer.finish().unwrap();
        let index = tabix::build(std::io::Cursor::new(&data), tabix::Layout::VCF, false).unwrap();
        let path = scratch.file("calls.vcf.gz", &data);
        scratch.file("calls.vcf.gz.tbi", &index);
        let indexed = open(path).unwrap();
        let wide = load(&indexed, "chr1", near, picked(0), 1, false, no_reference).unwrap();
        assert!(matches!(wide.content, Content::Signal(_)));
        let found = places(&wide);
        assert_eq!((found[0].0, found[1]), (99, (199, 1.0)));
        assert!((found[0].1 - 10.0 / 22.0).abs() < 1e-6);
        let Format::Variants(file) = &indexed.format else {
            panic!("expected variants");
        };
        let over = file.fractions_within(indexed.data(), ("chr1", near), 0, 0);
        assert_eq!(over, Ok(None));
        let off = load(
            &indexed,
            "chr1",
            near,
            Options::default(),
            1,
            true,
            no_reference,
        );
        assert_eq!(off.unwrap().fractions, None);
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
            assert_eq!(source.detail(), 1_000_000);
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
            let names = names(&source, &assembly(), None).unwrap();
            assert!(names.find("rs1001").is_some() || names.find("rs1002").is_some());
        }
    }

    fn wide(source: &Source, window: Region, step: u64, length: u64) -> Loaded {
        let scope = Scope {
            step,
            detail: false,
            rules: &[],
            length,
        };
        load_kept(
            source,
            "chr1",
            window,
            Options::default(),
            scope,
            no_reference,
        )
        .unwrap()
    }

    /// The records that densities per Mb stand for.
    fn records(summaries: &[genie_core::signal::Summary]) -> f64 {
        summaries
            .iter()
            .map(|summary| f64::from(summary.max) * summary.span.len() as f64 / 1e6)
            .sum()
    }

    #[test]
    fn vcf_files_show_a_density_when_the_view_is_wide() {
        for suffix in ["tbi", "csi"] {
            let scratch = Scratch::new(&format!("vcf-density-{suffix}"));
            let data = std::fs::read(fixture("variants.vcf.gz")).unwrap();
            let path = scratch.file("calls.vcf.gz", data);
            let index = std::fs::read(fixture(&format!("variants.vcf.gz.{suffix}"))).unwrap();
            scratch.file(&format!("calls.vcf.gz.{suffix}"), index);
            let source = open(path).unwrap();
            assert_eq!(source.span(), u64::MAX);
            assert_eq!(source.detail(), 1_000_000);
            assert!(source.has_density());
            let loaded = wide(&source, region(0, 2_000_000), 16_384, 3_000_000);
            let summaries = signal(&loaded);
            let total = records(summaries);
            assert!((total - 700.0).abs() < 1.0, "{suffix} {total}");
            let far: Vec<_> = summaries
                .iter()
                .filter(|summary| summary.span.start() >= 1_100_000)
                .copied()
                .collect();
            assert!(records(&far) < 1.0, "{suffix} {}", records(&far));
        }
        let scratch = Scratch::new("plain-vcf-density");
        let text = "#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\n\
                    chr1\t20\t.\tA\tG\t.\tPASS\t.\n\
                    chr1\t30\t.\tA\tG\t.\tPASS\t.\n\
                    chr1\t2000001\t.\tA\tG\t.\tPASS\t.\n";
        let source = open(scratch.file("calls.vcf", text)).unwrap();
        assert_eq!(source.span(), u64::MAX);
        let loaded = wide(&source, region(0, 4_000_000), 1_000_000, 4_000_000);
        let densities: Vec<f32> = signal(&loaded).iter().map(|summary| summary.max).collect();
        assert_eq!(densities, [2.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn feature_files_show_a_density_when_the_view_is_wide() {
        let source = open(fixture("genes.bed.gz")).unwrap();
        assert_eq!((source.span(), source.detail()), (u64::MAX, 10_000_000));
        let window = region(0, 3_000_000);
        let close = load(
            &source,
            "chr1",
            window,
            Options::default(),
            1,
            true,
            no_reference,
        );
        let Content::Features(annotation) = &close.unwrap().content else {
            panic!("expected features");
        };
        let count = annotation.features.len() as f64;
        let total = records(signal(&wide(&source, window, 65_536, 3_000_000)));
        assert!(
            count > 10.0 && (total - count).abs() < 1.0,
            "{count} {total}"
        );
        let scratch = Scratch::new("plain-bed-density");
        let text = "chr1\t10\t20\ta\nchr1\t1500000\t1500100\tb\nchr1\t1600000\t1600100\tc\n";
        let source = open(scratch.file("marks.bed", text)).unwrap();
        let loaded = wide(&source, region(0, 2_000_000), 1_000_000, 2_000_000);
        let densities: Vec<f32> = signal(&loaded).iter().map(|summary| summary.max).collect();
        assert_eq!(densities, [1.0, 2.0]);
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
            names(&source, &assembly(), None)
                .unwrap()
                .find("rs9")
                .unwrap()
                .contig,
            0
        );
    }

    fn variant_ids(source: &Source, contig: &str) -> Vec<String> {
        let loaded = load(
            source,
            contig,
            region(0, 10_000),
            Options::default(),
            1,
            true,
            no_reference,
        )
        .unwrap();
        let Content::Variants(calls) = loaded.content else {
            panic!("expected variants");
        };
        calls
            .variants
            .into_iter()
            .map(|variant| variant.id)
            .collect()
    }

    const GWAS: &str = "chromosome\tbase_pair_location\teffect_allele\tother_allele\tbeta\t\
                        p_value\trsid\n\
                        1\t1000\tA\tG\t0.1\t5e-9\trs1\n1\t2000\tC\tT\t-0.02\t0.2\trs2\n\
                        2\t500\tG\tA\t0.3\t1e-4\trs3\n";

    fn associations(source: &Source, contig: &str) -> Vec<(u64, bool, Option<String>)> {
        let loaded = load(
            source,
            contig,
            region(0, 10_000),
            Options::default(),
            1,
            true,
            no_reference,
        )
        .unwrap();
        let Content::Associations(found) = loaded.content else {
            panic!("expected associations");
        };
        found
            .into_iter()
            .map(|found| {
                (
                    found.position,
                    found.value > 7.3,
                    found.row.map(String::from),
                )
            })
            .collect()
    }

    #[test]
    fn gwas_files_open_whole_or_with_their_index() {
        let scratch = Scratch::new("gwas");
        let plain = open(scratch.file("height.tsv", GWAS)).unwrap();
        assert!(plain.has_associations());
        assert_eq!(plain.contigs(), ["1", "2"]);
        let found = associations(&plain, "1");
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].0, found[0].1), (999, true));
        let row = found[0].2.clone().unwrap();
        let association = genie_core::association::Association {
            position: 999,
            value: 8.3,
            row: Some(row.into()),
        };
        let labels: Vec<String> = super::association_fields(&plain, &association)
            .into_iter()
            .map(|field| field.label)
            .collect();
        assert_eq!(
            labels,
            ["effect_allele", "other_allele", "beta", "p_value", "rsid"]
        );
        let mut writer = genie_formats::bgzf::Writer::new(Vec::new());
        std::io::Write::write_all(&mut writer, GWAS.as_bytes()).unwrap();
        let data = writer.finish().unwrap();
        let layout = tabix::Layout::generic([1, 2, 2]).skipping(1);
        let index = tabix::build(std::io::Cursor::new(&data), layout, false).unwrap();
        let path = scratch.file("height.tsv.gz", &data);
        scratch.file("height.tsv.gz.tbi", &index);
        let indexed = open(path).unwrap();
        assert!(matches!(
            indexed.format,
            Format::Gwas(super::GwasFile::Indexed { .. })
        ));
        assert_eq!(associations(&indexed, "2").len(), 1);
        assert!(super::is_track(
            &PathBuf::from("/data/run.glm.linear").into()
        ));
    }

    #[test]
    fn tabix_tables_show_a_column_or_their_rows() {
        let scratch = Scratch::new("tabix-table");
        let text = "#Chrom\tPos\tRef\tAlt\tRawScore\tPHRED\n\
                    1\t101\tT\tA\t0.7\t8.5\n1\t101\tT\tC\t0.8\t9.5\n1\t102\tA\tG\t0.6\t7.5\n";
        let mut writer = genie_formats::bgzf::Writer::new(Vec::new());
        std::io::Write::write_all(&mut writer, text.as_bytes()).unwrap();
        let data = writer.finish().unwrap();
        let layout = tabix::Layout::generic([1, 2, 2]);
        let index = tabix::build(std::io::Cursor::new(&data), layout, false).unwrap();
        let path = scratch.file("scores.tsv.gz", &data);
        scratch.file("scores.tsv.gz.tbi", &index);
        let source = open(path).unwrap();
        assert!(source.has_columns());
        assert_eq!(source.contigs(), ["1"]);
        let shown = |pick| {
            let options = Options {
                table: pick,
                ..Options::default()
            };
            load(
                &source,
                "1",
                region(0, 1_000),
                options,
                1,
                true,
                no_reference,
            )
            .unwrap()
            .content
        };
        let Content::Signal(scores) = shown(Pick::Auto) else {
            panic!("expected a signal");
        };
        let highest: Vec<f32> = scores.summaries.iter().map(|summary| summary.max).collect();
        assert_eq!(highest, [8.5, 9.5, 7.5]);
        let Content::Signal(raw) = shown(Pick::Column(4)) else {
            panic!("expected a signal");
        };
        assert!((raw.summaries[0].max - 0.7).abs() < f32::EPSILON);
        let Content::Features(rows) = shown(Pick::Rows) else {
            panic!("expected features");
        };
        assert_eq!(rows.features.len(), 3);
        assert_eq!(rows.features[0].name(), "T");
        let fields: Vec<(String, String)> = feature_fields(&source, &rows.features[1])
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect();
        let wanted = [
            ("Ref", "T"),
            ("Alt", "C"),
            ("RawScore", "0.8"),
            ("PHRED", "9.5"),
        ];
        assert_eq!(
            fields,
            wanted.map(|(label, value)| (label.to_owned(), value.to_owned()))
        );
    }

    #[test]
    fn bcf_files_open_as_variant_tracks() {
        let source = open(fixture("calls.bcf")).unwrap();
        assert!(matches!(
            source.format,
            Format::Variants(VariantFile::Bcf { .. })
        ));
        assert_eq!(source.contigs(), ["chr1", "chr2"]);
        assert_eq!(source.samples(), ["A", "B"]);
        assert_eq!(variant_ids(&source, "chr1"), ["rs1", "", "sv1"]);
        assert_eq!(variant_ids(&source, "chr2"), ["rs4"]);
        let found = names(&source, &assembly(), None).unwrap();
        assert_eq!(found.find("rs4").unwrap().contig, 1);
        let scratch = Scratch::new("bcf-alone");
        let alone = scratch.file("alone.bcf", fs::read(fixture("calls.bcf")).unwrap());
        let whole = open(alone).unwrap();
        assert!(matches!(
            whole.format,
            Format::Variants(VariantFile::Table { .. })
        ));
        assert_eq!(variant_ids(&whole, "chr1"), ["rs1", "", "sv1"]);
    }

    fn contact_map(source: &Source, step: u64, contacts: Showing) -> Contacts {
        let options = Options {
            contacts,
            ..Options::default()
        };
        let loaded = load(
            source,
            "chr1",
            region(0, 3_000),
            options,
            step,
            true,
            no_reference,
        );
        let Content::Contacts(found) = loaded.unwrap().content else {
            panic!("expected contacts");
        };
        found
    }

    #[test]
    fn hic_files_open_as_contact_maps() {
        let source = open(fixture("contacts.hic")).unwrap();
        assert!(source.has_contacts());
        assert_eq!(source.contigs(), ["chr1", "chr2"]);
        let names = ["NONE".to_owned(), "KR".to_owned(), "SCALE".to_owned()];
        assert_eq!(source.normalizations(None), (&names[..], 2));
        assert_eq!(source.normalizations(Some(1)), (&names[..], 1));
        let best = contact_map(&source, 4, Showing::default());
        assert_eq!((best.map.bin, best.normalization.as_str()), (100, "KR"));
        assert_eq!(best.map.contacts.len(), 9);
        assert_eq!(best.map.value(0, 0), Some(1.0));
        let counts = Showing {
            normalization: Some(0),
            ratio: false,
        };
        assert_eq!(contact_map(&source, 4, counts).map.value(7, 10), Some(11.0));
        let ratios = contact_map(
            &source,
            4,
            Showing {
                ratio: true,
                ..counts
            },
        )
        .map;
        assert!(ratios.ratio);
        assert_eq!(
            (ratios.value(0, 0), ratios.value(7, 10)),
            (Some(0.25), Some(22.0))
        );
        let coarse = contact_map(&source, 64, Showing::default());
        assert_eq!(
            (coarse.map.bin, coarse.normalization.as_str()),
            (1_000, "SCALE")
        );
    }

    #[test]
    fn cooler_files_open_as_contact_maps() {
        let source = open(fixture("contacts.mcool")).unwrap();
        assert!(source.has_contacts());
        assert_eq!(source.contigs(), ["chr1", "chr2"]);
        let names = ["NONE".to_owned(), "weight".to_owned()];
        assert_eq!(source.normalizations(None), (&names[..], 1));
        let best = contact_map(&source, 4, Showing::default());
        assert_eq!(
            (best.map.bin, best.normalization.as_str()),
            (1_000, "weight")
        );
        assert_eq!(best.map.contacts.len(), 3);
        assert_eq!(best.map.value(0, 1), Some(10.0));
        assert_eq!(best.map.value(1, 1), Some(32.0));
        let counts = Showing {
            normalization: Some(0),
            ratio: false,
        };
        assert_eq!(contact_map(&source, 4, counts).map.value(0, 1), Some(5.0));
        let ratios = contact_map(
            &source,
            4,
            Showing {
                ratio: true,
                ..counts
            },
        )
        .map;
        assert_eq!(ratios.value(0, 1), Some(1.0));
        assert!((ratios.value(0, 0).unwrap() - 10.0 / 9.0).abs() < 1e-6);
        assert_eq!(contact_map(&source, 100, Showing::default()).map.bin, 2_000);
    }

    #[test]
    fn variant_fields_list_the_annotations_for_the_filters() {
        let scratch = Scratch::new("annotated-vcf");
        let text = "##INFO=<ID=CSQ,Number=.,Type=String,Description=\"Consequence annotations \
                    from Ensembl VEP. Format: Allele|Consequence|IMPACT|SYMBOL|SIFT\">\n\
                    #CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\n\
                    chr1\t20\t.\tC\tT\t.\tPASS\tDP=5;CSQ=T|stop_gained|HIGH|GENEA|deleterious(0)\n\
                    chr1\t30\t.\tC\tT\t.\tPASS\tDP=6;CSQ=T|intron_variant|MODIFIER|GENEB|\n";
        let source = open(scratch.file("calls.vcf", text)).unwrap();
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
        let Content::Variants(calls) = &loaded.content else {
            panic!("expected variants");
        };
        let shown: Vec<(String, String)> = variant_fields(&source, &calls.variants[0], &[], None)
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect();
        let pairs: Vec<(&str, &str)> = shown
            .iter()
            .map(|(label, value)| (label.as_str(), value.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("Filter", "PASS"),
                ("Consequence", "stop_gained"),
                ("Impact", "HIGH"),
                ("Gene", "GENEA"),
                ("CSQ SIFT", "deleterious(0)"),
                ("DP", "5"),
            ]
        );
        let high = Rule {
            label: "Impact".to_owned(),
            test: Test::Keep("HIGH".to_owned()),
        };
        let kept: Vec<bool> = calls
            .variants
            .iter()
            .map(|variant| {
                genie_formats::fields::passes(
                    std::slice::from_ref(&high),
                    &variant_fields(&source, variant, &[], None),
                )
            })
            .collect();
        assert_eq!(kept, [true, false]);
    }

    /// A review mark gives the fields Review and Review note first. A filter on them hides
    /// variants, and Next variant skips the variants that it hides.
    #[test]
    fn review_marks_give_fields_for_filters_and_jumps() {
        use genie_core::review::{Key, Review, Reviews, Verdict};
        let scratch = Scratch::new("reviewed-vcf");
        let text = "#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\n\
                    chr1\t20\trs20\tC\tT\t.\tPASS\tDP=5\n\
                    chr1\t30\t.\tC\tT\t.\tPASS\tDP=6\n\
                    chr1\t40\t.\tG\tA\t.\tPASS\tDP=7\n";
        let source = open(scratch.file("calls.vcf", text)).unwrap();
        let options = Options::default();
        let loaded = load(
            &source,
            "chr1",
            region(0, 100),
            options,
            1,
            true,
            no_reference,
        );
        let loaded = loaded.unwrap();
        let Content::Variants(calls) = &loaded.content else {
            panic!("expected variants");
        };
        let mut reviews = Reviews::default();
        let review = Review {
            verdict: Verdict::False,
            id: String::new(),
            note: "strand bias".to_owned(),
        };
        reviews.set(Key::of("chr1", &calls.variants[1]), Some(review));
        let marked = reviews.of("chr1", &calls.variants[1]);
        let fields: Vec<(String, String)> =
            variant_fields(&source, &calls.variants[1], &[], marked)
                .into_iter()
                .map(|field| (field.label, field.value))
                .collect();
        let review = |label: &str, value: &str| (label.to_owned(), value.to_owned());
        assert_eq!(
            fields[..2],
            [
                review("Review", "false"),
                review("Review note", "strand bias")
            ]
        );
        let plain = variant_fields(&source, &calls.variants[0], &[], None);
        assert!(plain.iter().all(|field| field.label != "Review"));
        let hide = Rule {
            label: "Review".to_owned(),
            test: Test::Hide("false".to_owned()),
        };
        let names = [Some("chr1".to_owned())];
        let assembly = Assembly::new(vec![genie_core::Contig {
            name: "chr1".to_owned(),
            length: 1_000,
        }]);
        let next = |rules: &[Rule]| {
            neighbor(
                &source,
                &names,
                &assembly,
                0,
                19,
                true,
                (rules, &[], &reviews),
            )
            .unwrap()
        };
        assert_eq!(next(std::slice::from_ref(&hide)), Some((0, region(39, 40))));
        assert_eq!(next(&[]), Some((0, region(29, 30))));
    }

    fn signal(loaded: &Loaded) -> &[genie_core::signal::Summary] {
        let Content::Signal(signal) = &loaded.content else {
            panic!("expected signal");
        };
        &signal.summaries
    }

    #[test]
    fn genbank_files_open_as_tracks_of_their_features() {
        let scratch = Scratch::new("genbank-track");
        let text = "\
LOCUS       NC_1          40 bp    DNA     linear   VRL 01-JAN-2020
VERSION     NC_1.1
FEATURES             Location/Qualifiers
     gene            3..20
                     /gene=\"abc\"
     CDS             3..20
                     /gene=\"abc\"
                     /product=\"abc protein\"
     promoter        25..30
                     /label=P1
                     /ApEinfo_fwdcolor=#ff0000
ORIGIN
        1 acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt
//
";
        let path = scratch.file("virus.gbk", text);
        assert!(is_track(&Location::File(path.clone())));
        let source = open(path).unwrap();
        assert_eq!(source.contigs(), ["NC_1.1"]);
        assert!(source.has_colors());
        let no_reference = |_: Region| Err(String::new());
        let options = Options::default();
        let loaded = load(
            &source,
            "NC_1.1",
            region(0, 40),
            options,
            1,
            true,
            no_reference,
        )
        .unwrap();
        let Content::Features(annotation) = &loaded.content else {
            panic!("features");
        };
        let shown: Vec<&str> = annotation.features.iter().map(Feature::name).collect();
        assert_eq!(shown, ["abc", "P1"]);
        let fields: Vec<(String, String)> = feature_fields(&source, &annotation.features[0])
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect();
        assert_eq!(
            fields,
            [
                ("type".to_owned(), "CDS".to_owned()),
                ("gene".to_owned(), "abc".to_owned()),
                ("product".to_owned(), "abc protein".to_owned()),
            ]
        );
        let assembly = Assembly::new(vec![Contig {
            name: "NC_1.1".to_owned(),
            length: 40,
        }]);
        let found = names(&source, &assembly, None).unwrap();
        assert_eq!(
            found.find("P1").map(|named| named.span),
            Some(region(24, 30))
        );
        let density = load(
            &source,
            "NC_1.1",
            region(0, 40),
            options,
            10,
            false,
            no_reference,
        );
        assert!(matches!(density.unwrap().content, Content::Signal(_)));
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
        assert_eq!(names(&source, &assembly(), None).unwrap(), Names::default());
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
        assert_eq!(names(&source, &assembly(), None).unwrap(), Names::default());
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
                        let found = neighbor(
                            &source,
                            &names,
                            &assembly,
                            contig,
                            position,
                            forward,
                            (&[], &[], &genie_core::review::Reviews::default()),
                        )
                        .unwrap();
                        let expected = scanned(&source, &names, contig, position, forward);
                        assert_eq!(found, expected, "{file} {contig} {position} {forward}");
                    }
                }
            }
        }
    }

    #[test]
    fn neighbors_skip_features_that_the_filter_hides() {
        let scratch = Scratch::new("filtered-neighbors");
        let line = |contig: &str, start: u64, name: &str, mark: &str| {
            let end = start + 100;
            format!(
                "{contig}\t{start}\t{end}\t{name}\t0\t+\t{start}\t{end}\t0\t1\t100,\t0,\t{mark}\n"
            )
        };
        let text = [
            line("chr1", 100, "a", "keep"),
            line("chr1", 300, "b", "drop"),
            line("chr1", 500, "c", "keep"),
            line("chr2", 100, "d", "drop"),
            line("chr2", 300, "e", "keep"),
        ]
        .concat();
        let source = open(scratch.file("marked.bed", text)).unwrap();
        let assembly = three_contigs();
        let names = contig_names(source.contigs(), &assembly);
        let keep = [Rule {
            label: "Column 13".to_owned(),
            test: Test::Keep("keep".to_owned()),
        }];
        let next = |contig, position, forward, rules: &[Rule]| {
            neighbor(
                &source,
                &names,
                &assembly,
                contig,
                position,
                forward,
                (rules, &[], &genie_core::review::Reviews::default()),
            )
            .unwrap()
        };
        assert_eq!(next(0, 150, true, &[]), Some((0, region(300, 400))));
        assert_eq!(next(0, 150, true, &keep), Some((0, region(500, 600))));
        assert_eq!(next(0, 550, true, &keep), Some((1, region(300, 400))));
        assert_eq!(next(1, 350, false, &keep), Some((1, region(300, 400))));
        assert_eq!(next(1, 150, false, &keep), Some((0, region(500, 600))));
        assert_eq!(next(1, 350, true, &keep), None);
    }

    #[test]
    fn zoomed_out_bigbed_density_shows_only_kept_features() {
        let source = open(fixture("ccres.bb")).unwrap();
        let Format::Features(file) = &source.format else {
            panic!("expected features");
        };
        let class = [Rule {
            label: "Class".to_owned(),
            test: Test::Keep("CTCF-only".to_owned()),
        }];
        let whole = region(0, 20_000);
        let density = |rules: &[Rule], budget| {
            let scope = Scope {
                step: 16,
                detail: false,
                rules,
                length: 20_000,
            };
            let loaded = file
                .load(source.data(), "chr1", whole, scope, budget)
                .unwrap();
            let Content::Signal(signal) = loaded.content else {
                panic!("expected a density");
            };
            signal
        };
        let kept = density(&class, 1 << 24);
        let covered: u64 = kept
            .summaries
            .iter()
            .map(|summary| summary.span.len())
            .sum();
        assert_eq!(covered, 400);
        assert!(
            kept.summaries
                .iter()
                .all(|summary| summary.span.start() >= 500 && summary.span.end() <= 900)
        );
        assert_eq!(
            kept.kept,
            Some(Kept {
                kept: vec![region(500, 900)],
                all: vec![region(100, 400), region(500, 900)],
            })
        );
        assert_eq!(density(&class, 1).kept, None);
        assert_eq!(density(&[], 1 << 24).kept, None);
    }

    #[test]
    fn hidden_names_fail_every_rule_check() {
        let scratch = Scratch::new("hidden-names");
        let text = [
            "chr1\t100\t200\tTP53\t0\t+\t100\t200\t0\t1\t100,\t0,\tdrop\n",
            "chr1\t300\t400\tBRCA1\t0\t+\t300\t400\t0\t1\t100,\t0,\tkeep\n",
        ]
        .concat();
        let source = open(scratch.file("named.bed", text)).unwrap();
        let keep = [Rule {
            label: "Column 13".to_owned(),
            test: Test::Keep("keep".to_owned()),
        }];
        assert!(hides(
            &source,
            "chr1",
            region(100, 200),
            "tp53",
            (&keep, &[], &genie_core::review::Reviews::default())
        ));
        assert!(!hides(
            &source,
            "chr1",
            region(300, 400),
            "BRCA1",
            (&keep, &[], &genie_core::review::Reviews::default())
        ));
        assert!(!hides(
            &source,
            "chr1",
            region(100, 200),
            "TP53",
            (&[], &[], &genie_core::review::Reviews::default())
        ));
        assert!(!hides(
            &source,
            "chr1",
            region(500, 600),
            "EGFR",
            (&keep, &[], &genie_core::review::Reviews::default())
        ));
    }

    #[test]
    fn neighbors_start_after_the_position_and_cross_contigs() {
        let assembly = three_contigs();
        let genes = open(fixture("genes.bed.gz")).unwrap();
        let names = contig_names(genes.contigs(), &assembly);
        let next = |position, forward| {
            neighbor(
                &genes,
                &names,
                &assembly,
                0,
                position,
                forward,
                (&[], &[], &genie_core::review::Reviews::default()),
            )
        };
        assert_eq!(next(0, true).unwrap(), Some((0, region(1_050, 3_208))));
        let after = next(1_050, true).unwrap().unwrap();
        assert_eq!((after.0, after.1.start()), (0, 1_415));
        assert_eq!(next(1_050, false).unwrap(), None);
        let calls = open(fixture("variants.vcf.gz")).unwrap();
        let names = contig_names(calls.contigs(), &assembly);
        let first = neighbor(
            &calls,
            &names,
            &assembly,
            0,
            0,
            true,
            (&[], &[], &genie_core::review::Reviews::default()),
        )
        .unwrap();
        assert_eq!(first, Some((0, region(1_000, 1_001))));
        assert_eq!(
            neighbor(
                &calls,
                &names,
                &assembly,
                2,
                0,
                true,
                (&[], &[], &genie_core::review::Reviews::default())
            )
            .unwrap(),
            None
        );
        let last = neighbor(
            &calls,
            &names,
            &assembly,
            2,
            0,
            false,
            (&[], &[], &genie_core::review::Reviews::default()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(last.0, 1);
        let reads = open(fixture("reads.bam")).unwrap();
        let names = contig_names(reads.contigs(), &assembly);
        assert_eq!(
            neighbor(
                &reads,
                &names,
                &assembly,
                0,
                0,
                true,
                (&[], &[], &genie_core::review::Reviews::default())
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn hub_tracks_open_by_their_type_without_known_names() {
        let read = |name: &str| fs::read(fixture(name)).unwrap();
        let files = vec![
            ("/data/1".to_owned(), read("genes.bb")),
            ("/data/2".to_owned(), read("signal.bw")),
            ("/data/3".to_owned(), read("reads.bam")),
            ("/data/3i".to_owned(), read("reads.bam.bai")),
            ("/data/4".to_owned(), read("variants.vcf.gz")),
            ("/data/4i".to_owned(), read("variants.vcf.gz.tbi")),
            ("/data/5".to_owned(), read("cram30.cram")),
            ("/data/5i".to_owned(), read("cram30.cram.crai")),
        ];
        let server = crate::server::serve(files);
        let at = |path: &str| Location::parse(&server.url(path)).unwrap();
        let open = |path: &str, kind: &str, index: Option<&str>| {
            let typed = Typed::of(kind).unwrap();
            open_typed(at(path), typed, index.map(at).as_ref())
        };
        assert!(open("/data/1", "bigGenePred", None).unwrap().is_bigbed());
        assert!(open("/data/2", "bigWig", None).unwrap().has_signal());
        assert!(
            open("/data/3", "bam", Some("/data/3i"))
                .unwrap()
                .has_reads()
        );
        let calls = open("/data/4", "vcfTabix", Some("/data/4i")).unwrap();
        assert!(calls.has_variants());
        let cram = open("/data/5", "cram", Some("/data/5i")).unwrap();
        assert!(matches!(
            &cram.format,
            Format::Reads(file) if matches!(file.index, Reads::Cram(..))
        ));
        assert_eq!(cram.location, at("/data/5"));
        let missing = open("/data/3", "bam", None).unwrap_err();
        assert!(missing.contains("There is no index 3.bai"), "{missing}");
        assert_eq!(Typed::of("hic"), Some(Typed::Hic));
        assert_eq!(Typed::of("longTabix"), None);
        assert_eq!(Typed::of("bigInteract"), Some(Typed::Big));
        assert_eq!(Typed::of("vcfPhasedTrio"), Some(Typed::Vcf));
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
        assert_eq!(names(&remote, &assembly, None).unwrap(), Names::default());
        assert_ne!(names(&local, &assembly, None).unwrap(), Names::default());
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

    fn links_of(loaded: &Loaded) -> Vec<String> {
        let Content::Links(found) = &loaded.content else {
            panic!("expected links");
        };
        found.iter().map(|link| link.name.clone()).collect()
    }

    #[test]
    fn interaction_files_open_as_link_tracks() {
        let scratch = Scratch::new("track-links");
        let big = open(fixture("interact.bb")).unwrap();
        assert!(big.has_links() && big.has_colors() && !big.can_jump());
        assert_eq!(big.contigs(), ["chr1", "chr2"]);
        let window = region(0, 1_000_000);
        let none = |_: Region| Err("There is no reference.".to_owned());
        let loaded = load(&big, "chr1", window, Options::default(), 1, true, none).unwrap();
        assert_eq!(links_of(&loaded), ["loopA", "loopB", "trans", ""]);
        let Content::Links(found) = &loaded.content else {
            panic!("expected links");
        };
        let labels: Vec<String> = link_fields(&big, &found[0])
            .into_iter()
            .map(|field| field.label)
            .collect();
        assert_eq!(labels[0], "Experiment name");
        let text = "#chr1\tx1\tx2\tchr2\ty1\ty2\tname\tscore\tstrand1\tstrand2\tcolor\tobserved\n\
            chr1\t1000\t2000\tchr1\t90000\t91000\tloop1\t5\t.\t.\t0,0,255\t30\n";
        let path = scratch.file("loops.bedpe", text);
        assert!(is_track(&Location::File(path.clone())));
        assert!(is_track(&Location::File(PathBuf::from(
            "/data/loops.bedpe.gz"
        ))));
        let table = open(path).unwrap();
        assert!(table.has_links());
        let loaded = load(
            &table,
            "chr1",
            region(40_000, 50_000),
            Options::default(),
            1,
            true,
            none,
        )
        .unwrap();
        assert_eq!(links_of(&loaded), ["loop1"]);
        let Content::Links(found) = &loaded.content else {
            panic!("expected links");
        };
        let fields: Vec<(String, String)> = link_fields(&table, &found[0])
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect();
        assert_eq!(fields, [("observed".to_owned(), "30".to_owned())]);
    }

    fn junctions_of(loaded: &Loaded) -> &Splices {
        let Content::Junctions(found) = &loaded.content else {
            panic!("expected junctions");
        };
        found
    }

    #[test]
    fn junction_files_open_as_junction_tracks() {
        let scratch = Scratch::new("track-junctions");
        let none = |_: Region| Err("There is no reference.".to_owned());
        let star = "chr1\t1001\t2000\t1\t1\t1\t12\t3\t40\nchr1\t5001\t9000\t2\t2\t0\t4\t0\t25\n";
        let path = scratch.file("liver.SJ.out.tab", star);
        assert!(is_track(&Location::File(path.clone())));
        let source = open(path).unwrap();
        assert!(source.has_junctions() && !source.has_links() && !source.can_jump());
        assert_eq!(source.contigs(), ["chr1"]);
        let loaded = load(
            &source,
            "chr1",
            region(0, 3_000),
            Options::default(),
            1,
            true,
            none,
        )
        .unwrap();
        let found = junctions_of(&loaded);
        assert_eq!(found.junctions.len(), 1);
        assert_eq!(found.junctions[0].span, region(1_000, 2_000));
        assert_eq!(found.junctions[0].count, 12);
        assert_eq!(found.fields[0][0].label, "Unique reads");
        let bed = "track name=junctions description=\"TopHat junctions\"\n\
            chr2\t100\t400\tJUNC1\t7\t+\t100\t400\t0\t2\t50,50\t0,250\n";
        for name in ["sample_junctions.bed", "spliced.bed"] {
            let source = open(scratch.file(name, bed)).unwrap();
            assert!(source.has_junctions(), "{name}");
            let loaded = load(
                &source,
                "chr2",
                region(0, 1_000),
                Options::default(),
                1,
                true,
                none,
            )
            .unwrap();
            assert_eq!(junctions_of(&loaded).junctions[0].span, region(150, 350));
        }
        let genes = open(scratch.file("genes.bed", "chr2\t100\t400\tGENE1\n")).unwrap();
        assert!(!genes.has_junctions());
    }

    /// A view shows the junctions with the most reads when a file has very many in view.
    #[test]
    fn crowded_views_keep_the_junctions_with_the_most_reads() {
        use std::fmt::Write as _;
        let scratch = Scratch::new("track-junctions-many");
        let mut text = String::new();
        for at in 0..(junctions::MOST + 10) {
            let start = 1_000 + at as u64 * 10;
            let _ = writeln!(text, "chr1\t{start}\t{}\t1\t1\t0\t{at}\t0\t30", start + 500);
        }
        let source = open(scratch.file("many.SJ.out.tab", &text)).unwrap();
        let none = |_: Region| Err("There is no reference.".to_owned());
        let window = region(0, 1_000_000);
        let loaded = load(&source, "chr1", window, Options::default(), 1, true, none).unwrap();
        let found = junctions_of(&loaded);
        assert_eq!(found.junctions.len(), junctions::MOST);
        assert!(found.junctions.iter().all(|junction| junction.count >= 10));
        assert!(
            found
                .junctions
                .windows(2)
                .all(|pair| pair[0].span.start() <= pair[1].span.start())
        );
        assert_eq!(found.fields.len(), found.junctions.len());
    }

    #[test]
    fn segment_files_open_as_heatmap_tracks_of_their_samples() {
        let scratch = Scratch::new("track-segments");
        let text = "ID\tchrom\tloc.start\tloc.end\tnum.mark\tseg.mean\n\
            tumor1\tchr1\t1\t1000\t12\t0.85\n\
            tumor2\tchr1\t101\t2000\t8\t-1.2\n";
        let path = scratch.file("cohort.seg", text);
        for name in [
            "/data/cohort.seg",
            "/data/t.cns",
            "/data/t.cnr",
            "/data/x.seg.gz",
        ] {
            assert!(is_track(&Location::File(PathBuf::from(name))), "{name}");
        }
        let source = open(path).unwrap();
        assert!(source.has_segments() && !source.can_jump());
        assert_eq!(source.samples(), ["tumor1", "tumor2"]);
        assert_eq!(source.segment_column(), "seg.mean");
        let none = |_: Region| Err("There is no reference.".to_owned());
        let window = region(500, 600);
        let loaded = load(&source, "chr1", window, Options::default(), 1, true, none).unwrap();
        let Content::Segments(copies) = &loaded.content else {
            panic!("expected segments");
        };
        assert_eq!(copies.samples, 2);
        let values: Vec<f32> = copies
            .segments
            .iter()
            .map(|segment| segment.value)
            .collect();
        assert_eq!(values, [0.85, -1.2]);
        let mut track = Track::new(1, Arc::new(source), &assembly());
        track.slot.wanted = Some((0, window));
        track.accept_at(0, 0, window, Ok(Arc::new(loaded)));
        assert_eq!(track.mode_at(0, 0), Mode::Expanded);
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
        let sources = [(source, None)];
        let found = lookup(&sources, "chr2_450", &assembly).unwrap();
        assert_eq!((found.contig, found.span), (1, region(281_270, 283_541)));
        let upper = lookup(&sources, "CHR2_450", &assembly).unwrap();
        assert_eq!(upper, found);
        assert_eq!(lookup(&sources, "missing", &assembly), None);
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
            data: None,
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
        let names = names(&source, &assembly(), None).unwrap();
        assert_eq!(names.find("geneb").unwrap().contig, 1);
    }

    #[test]
    fn paf_files_show_alignments_with_the_place_on_the_query() {
        let scratch = Scratch::new("paf");
        let text = "contig_9\t9000\t100\t2100\t-\tchr1\t1000\t100\t900\t760\t800\t60\ttp:A:P\tcg:Z:200M100D500M\n";
        let source = open(scratch.file("asm.paf", text)).unwrap();
        assert!(matches!(
            source.format,
            Format::Features(FeatureFile::Table {
                kind: Kind::Paf,
                ..
            })
        ));
        let loaded = load(
            &source,
            "chr1",
            region(0, 1_000),
            Options::default(),
            1,
            true,
            no_reference,
        )
        .unwrap();
        let Content::Features(annotation) = &loaded.content else {
            panic!("no features");
        };
        let feature = &annotation.features[0];
        assert_eq!(feature.name(), "contig_9");
        assert_eq!(feature.blocks(), [region(100, 300), region(400, 900)]);
        let fields = feature_fields(&source, feature);
        assert_eq!(fields[0].value, "contig_9:101-2100");
        assert!(
            fields
                .iter()
                .any(|field| field.label == "Identity (%)" && field.value == "95.0")
        );
        let names = names(&source, &assembly(), None).unwrap();
        assert_eq!(names.find("contig_9").unwrap().contig, 0);
    }

    #[test]
    fn plain_feature_files_need_no_second_read_for_names() {
        let scratch = Scratch::new("plain-names");
        let gff = "chr1\t.\tgene\t11\t20\t.\t+\t.\tID=g1;Name=geneA\n";
        for (name, text) in [("genes.bed", "chr1\t10\t20\tgeneA\n"), ("genes.gff3", gff)] {
            let path = scratch.file(name, text);
            let source = open(path.clone()).unwrap();
            std::fs::remove_file(&path).unwrap();
            let names = names(&source, &assembly(), None).unwrap();
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
    fn prepared_files_open_from_their_copy_and_their_index() {
        let scratch = Scratch::new("prepared-copy");
        let copy = scratch.file("copy.gff3.gz", fs::read(fixture("genes.gff3.gz")).unwrap());
        let index = scratch.file("copy.tbi", fs::read(fixture("genes.gff3.gz.tbi")).unwrap());
        let prepared = Prepared {
            data: Location::File(copy),
            index: Location::File(index),
        };
        let source = open_with(PathBuf::from("/data/genes.gff3"), Some(prepared)).unwrap();
        assert_eq!(source.location, PathBuf::from("/data/genes.gff3").into());
        assert_eq!(source.name, "genes.gff3");
        let window = region(50_000, 51_000);
        let loaded = load(
            &source,
            "chr1",
            window,
            Options::default(),
            1,
            true,
            no_reference,
        );
        let Content::Features(annotation) = &loaded.unwrap().content else {
            panic!("expected features");
        };
        assert!(
            annotation
                .features
                .iter()
                .any(|feature| feature.name() == "GENE1003")
        );
        let names = names(&source, &assembly(), None).unwrap();
        assert!(names.find("gene1003").is_some());
    }

    #[test]
    fn prepared_indexes_can_sit_away_from_the_reads() {
        let scratch = Scratch::new("prepared-index");
        let reads = scratch.file("reads.bam", fs::read(fixture("reads.bam")).unwrap());
        let index = scratch.file("cached.bai", fs::read(fixture("reads.bam.bai")).unwrap());
        assert!(open(reads.clone()).is_err());
        let prepared = Prepared {
            data: Location::File(reads.clone()),
            index: Location::File(index),
        };
        let source = open_with(reads, Some(prepared)).unwrap();
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
        );
        let Content::Reads(pile) = &loaded.unwrap().content else {
            panic!("expected reads");
        };
        assert!(pile.reads.len() >= 10, "{}", pile.reads.len());
    }

    const METHYL: &str = "chr1\t100\t101\tm\t20\t+\t100\t101\t255,0,0\t20\t75.00\t15\t5\t0\t0\t0\t0\t0\n\
                          chr1\t101\t102\tm\t10\t-\t101\t102\t255,0,0\t10\t50.00\t5\t5\t0\t0\t0\t0\t0\n\
                          chr1\t100\t101\th\t20\t+\t100\t101\t255,0,0\t20\t10.00\t2\t18\t0\t0\t0\t0\t0\n";

    fn methyl_values(source: &Source, options: Options) -> Vec<(u64, f64)> {
        let loaded = load(
            source,
            "chr1",
            region(0, 1_000),
            options,
            1,
            true,
            no_reference,
        );
        let Content::Signal(signal) = &loaded.unwrap().content else {
            panic!("expected signal");
        };
        signal
            .summaries
            .iter()
            .map(|summary| (summary.span.start(), (summary.sum * 10.0).round() / 10.0))
            .collect()
    }

    /// Juicer and `HiCCUPS` write their loops to `.txt` files, so their header line tells them.
    #[test]
    fn loop_lists_in_text_files_open_as_links() {
        let scratch = Scratch::new("loop-lists");
        let text = "chr1\tx1\tx2\tchr2\ty1\ty2\tcolor\to\n1\t10\t20\t1\t50\t60\t0,0,255\t5\n";
        let source = open(scratch.file("GM12878_looplist.txt", text)).unwrap();
        assert!(source.has_links());
        assert_eq!(source.contigs(), ["1"]);
        let other = open(scratch.file("notes.txt", "some words\n"));
        assert!(other.is_err());
    }

    #[test]
    fn bedmethyl_files_open_as_percent_signal() {
        let scratch = Scratch::new("bedmethyl");
        let source = open(scratch.file("calls.bedMethyl", METHYL)).unwrap();
        assert!(source.is_bedmethyl());
        assert_eq!(source.modifications(), ["5mC", "5hmC"]);
        assert_eq!(methyl_values(&source, Options::default()), [(100, 66.7)]);
        let separate = Options {
            methyl: Methyl {
                code: 0,
                separate: true,
            },
            ..Options::default()
        };
        assert_eq!(methyl_values(&source, separate), [(100, 75.0), (101, 50.0)]);
        let hydroxy = Options {
            methyl: Methyl {
                code: 1,
                separate: false,
            },
            ..Options::default()
        };
        assert_eq!(methyl_values(&source, hydroxy), [(100, 10.0)]);
        let track = Track::new(1, Arc::new(source), &assembly());
        assert_eq!(
            track.plot.range,
            Some(Scale {
                min: 0.0,
                max: 100.0
            })
        );
        let named_bed = open(scratch.file("calls.bed", METHYL)).unwrap();
        assert!(named_bed.is_bedmethyl());
        let plain_bed = open(scratch.file("genes.bed", "chr1\t10\t20\tgeneA\n")).unwrap();
        assert!(!plain_bed.is_bedmethyl());
        assert!(is_track(&PathBuf::from("/data/calls.bedMethyl.gz").into()));
    }

    #[test]
    fn bedmethyl_tracks_pick_the_modification_and_the_strands() {
        let scratch = Scratch::new("bedmethyl-change");
        let source = Arc::new(open(scratch.file("calls.bedMethyl", METHYL)).unwrap());
        let mut tracks = vec![Track::new(1, source.clone(), &assembly())];
        assert_eq!(tracks[0].modification_names(), ["5mC", "5hmC"]);
        assert_eq!(tracks[0].shown_modification().as_deref(), Some("5mC"));
        change(&mut tracks, 1, Change::Modification(1));
        assert_eq!(tracks[0].options.methyl.code, 1);
        assert_eq!(tracks[0].modification.as_deref(), Some("5hmC"));
        assert_eq!(tracks[0].shown_modification().as_deref(), Some("5hmC"));
        assert!(tracks[0].options.is_on(Toggle::Join));
        change(&mut tracks, 1, Change::Toggle(Toggle::Join));
        assert!(tracks[0].options.methyl.separate);
        assert!(!tracks[0].options.is_on(Toggle::Join));
        let options = tracks[0].options;
        let loaded = load(
            &source,
            "chr1",
            region(0, 1_000),
            options,
            1,
            true,
            no_reference,
        );
        assert_eq!(loaded.unwrap().options, options);
    }

    #[test]
    fn cram_tracks_load_the_same_reads_as_bam() {
        let reference =
            crate::reference::open(fixture("reference.fa.gz"), None, &mut |_| {}).unwrap();
        let cram = open(fixture("cram30.cram")).unwrap();
        let bam = open(fixture("cram.bam")).unwrap();
        assert_eq!(cram.contigs(), ["chr1", "chr2", "chrM"]);
        assert!(is_track(&PathBuf::from("a.cram").into()));
        for (contig, name, start, end) in [(0, "chr1", 10_000, 14_000), (1, "chr2", 0, 3_000)] {
            let window = region(start, end);
            let bases = |window: Region| reference.fetch(contig, window);
            let without_edits = |mut loaded: Loaded| {
                if let Content::Reads(pile) = &mut loaded.content {
                    for read in &mut pile.reads {
                        let tags = &mut read.alignment.tags.all;
                        tags.retain(|(name, _)| !matches!(name, b"NM" | b"MD"));
                        tags.sort();
                    }
                }
                loaded
            };
            let from_cram = load(&cram, name, window, Options::default(), 1, true, bases);
            let from_bam = load(&bam, name, window, Options::default(), 1, true, bases);
            let from_cram = without_edits(from_cram.unwrap());
            assert_eq!(from_cram, without_edits(from_bam.unwrap()), "{name}");
            let Content::Reads(pile) = &from_cram.content else {
                panic!("expected reads");
            };
            assert!(pile.count > 0, "{name}");
        }
    }

    #[test]
    fn cram_files_name_what_genie_cannot_read() {
        let scratch = Scratch::new("cram-errors");
        let bare = scratch.file("bare.cram", fs::read(fixture("cram30.cram")).unwrap());
        let error = open(bare).unwrap_err();
        assert_eq!(
            error,
            "There is no index bare.cram.crai. Index the file with samtools index."
        );
        let reference = |window: Region| Ok(vec![b'A'; window.len() as usize]);
        let cram = open(fixture("cram30.cram")).unwrap();
        let error = load(
            &cram,
            "chr1",
            region(10_000, 14_000),
            Options::default(),
            1,
            true,
            reference,
        )
        .unwrap_err();
        assert_eq!(
            error,
            "The reference is not the reference of the CRAM file here."
        );
        let newer = open(fixture("cram-codecs.cram")).unwrap();
        let error = load(
            &newer,
            "chr1",
            region(10_000, 14_000),
            Options::default(),
            1,
            true,
            reference,
        )
        .unwrap_err();
        assert_eq!(
            error,
            "The file uses CRAM 3.1 arithmetic, fqzcomp or name tokenizer compression, which genie does not read yet."
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

    #[test]
    fn quick_filters_show_or_hide_kinds_of_reads_and_low_mapq() {
        let source = open(fixture("reads.bam")).unwrap();
        let reference = |window: Region| Ok(vec![b'A'; window.len() as usize]);
        let reads = |options: Options| {
            let loaded = load(
                &source,
                "chr1",
                region(0, 50_000),
                options,
                1,
                true,
                reference,
            )
            .unwrap();
            let Content::Reads(pile) = loaded.content else {
                panic!("expected reads");
            };
            pile.reads
        };
        let plain = Options {
            downsample: false,
            ..Options::default()
        };
        let count = |options: Options, flag: u16| {
            reads(options)
                .iter()
                .filter(|read| read.alignment.has(flag))
                .count()
        };
        assert_eq!(count(plain, SECONDARY), 0);
        assert_eq!(count(plain, QC_FAIL), 0);
        let shown = Options {
            kinds: Kinds {
                secondary: true,
                failed: true,
                ..Kinds::default()
            },
            ..plain
        };
        assert!(count(shown, SECONDARY) > 0);
        assert!(count(shown, QC_FAIL) > 0);
        let all = reads(plain).len();
        let floor = |mapq| {
            reads(Options { mapq, ..plain })
                .iter()
                .all(|read| read.alignment.mapq >= mapq)
        };
        assert!(floor(30));
        assert!(reads(Options { mapq: 255, ..plain }).len() < all);
        let without = Options {
            kinds: Kinds {
                supplementary: false,
                ..Kinds::default()
            },
            ..plain
        };
        let supplementary = genie_core::alignment::SUPPLEMENTARY;
        assert_eq!(count(without, supplementary), 0);
    }

    #[test]
    fn read_fields_hold_mapq_the_insert_and_the_tags_and_rules_filter_reads() {
        let mut read = alignment(100, vec![(Op::Match, 50)]);
        read.flags = PAIRED;
        read.mate = Some(Mate {
            contig: 0,
            start: 400,
            insert: -350,
        });
        read.tags.all = vec![(*b"NM", "2".to_owned()), (*b"CB", "AAAC-1".to_owned())];
        let fields: Vec<(String, String)> = super::read_fields(&read)
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect();
        let expected = [
            ("MAPQ", "60"),
            ("Insert size", "350"),
            ("NM", "2"),
            ("CB", "AAAC-1"),
        ];
        let expected: Vec<(String, String)> = expected
            .iter()
            .map(|(label, value)| ((*label).to_owned(), (*value).to_owned()))
            .collect();
        assert_eq!(fields, expected);
        let source = open(fixture("reads.bam")).unwrap();
        let reference = |window: Region| Ok(vec![b'A'; window.len() as usize]);
        let count = |rules: &[Rule]| {
            let scope = Scope {
                step: 1,
                detail: true,
                rules,
                length: 8_000,
            };
            let loaded = super::load_kept(
                &source,
                "chr1",
                region(5_000, 8_000),
                Options::default(),
                scope,
                reference,
            )
            .unwrap();
            let Content::Reads(pile) = &loaded.content else {
                panic!("expected reads");
            };
            pile.reads.len()
        };
        let all = count(&[]);
        assert!(all >= 10, "{all}");
        let high = Rule {
            label: "MAPQ".to_owned(),
            test: Test::AtLeast(255.0),
        };
        assert_eq!(count(&[high]), 0);
        let low = Rule {
            label: "MAPQ".to_owned(),
            test: Test::AtMost(255.0),
        };
        assert_eq!(count(&[low]), all);
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

    /// When a read track shows pairs, the two mates of a pair share a row, and the pair takes the
    /// room from the start of one mate to the end of the other.
    #[test]
    fn pairs_put_the_two_mates_of_a_pair_on_one_row() {
        let mate = |name: &str, start| Alignment {
            name: name.to_owned(),
            flags: PAIRED,
            ..alignment(start, vec![(Op::Match, 10)])
        };
        let reads = vec![
            mate("A", 10),
            mate("B", 15),
            alignment(30, vec![(Op::Match, 10)]),
            mate("A", 50),
            mate("B", 70),
        ];
        let options = Options {
            mates: true,
            ..Options::default()
        };
        let Content::Reads(pile) = piled(region(0, 100), reads, options).content else {
            panic!("expected reads");
        };
        assert!(pile.mates);
        let rows_of = |rows: &[usize]| {
            let row = |name: &str, start: u64| {
                let place = pile
                    .reads
                    .iter()
                    .position(|read| read.alignment.name == name && read.alignment.start == start);
                rows[place.unwrap()]
            };
            (
                [row("A", 10), row("A", 50)],
                [row("B", 15), row("B", 70)],
                row("read30", 30),
            )
        };
        for rows in [pile.rows.clone(), pile.packed(region(0, 100)).0] {
            let ([a, a_mate], [b, b_mate], single) = rows_of(&rows);
            assert_eq!(a, a_mate);
            assert_eq!(b, b_mate);
            assert!(a != b && single != a && single != b, "{rows:?}");
        }
        assert!(
            Options::default()
                .toggled(Toggle::Mates)
                .is_on(Toggle::Mates)
        );
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
        let error = open(PathBuf::from("/data/notes.docx")).unwrap_err();
        assert_eq!(
            error,
            "The file notes.docx is not a BED, GFF3, GTF, bigBed, VCF, BCF, BAM, CRAM, bigWig, \
             bedGraph, bedMethyl, BEDPE, bigInteract, Hi-C or copy number file."
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
