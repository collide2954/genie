use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use genie_core::review::Reviews;
use genie_core::samples::Choice;
use genie_formats::fields::{self, Rule, Test};
use genie_formats::http::Url;
use genie_formats::ini::{self, Document, Section};

use genie_render::alignments::Look;
use genie_render::features::Mode;
use genie_render::scene::Hue;
use genie_render::signal::{Operation, Scale, Style};

use crate::export::Figure;
use crate::location::Location;
use crate::reading::{Frames, Reading};
use crate::track::{
    Declared, Filter, Grouping, Kinds, Methyl, Options, Pick, Plot, Showing, tag, tag_name,
};

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
    /// The file of the contig sizes of the reference, as the `chromSizes` file of a hub genome.
    pub sizes: Option<Location>,
    pub genome: Option<String>,
    pub locus: Option<String>,
    pub reading: Reading,
    pub tracks: Vec<SavedTrack>,
    pub regions: Vec<SavedRegion>,
    /// The track hubs that the session shows in the Tracks menu.
    pub hubs: Vec<Location>,
    /// The sample table of the session.
    pub samples: Option<Location>,
    /// The title and the parts of exported images.
    pub figure: Figure,
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
    /// The place of the other track of the comparison of a signal track, and the operation.
    pub compare: Option<(usize, Operation)>,
    pub mode: Option<Mode>,
    pub hue: Hue,
    pub height: Option<f32>,
    pub options: Options,
    pub plot: Plot,
    pub filter: Filter,
    pub modification: Option<String>,
    /// The normalization that somebody chose for a Hi-C map.
    pub normalization: Option<String>,
    pub declared: Option<Declared>,
    /// The fields of the sample table that group, sort and color the samples of the track.
    pub choice: Choice,
    /// The chain file that moves the data of the track to the assembly in view.
    pub lift: Option<Location>,
    /// A hidden track does not show and does not load its data.
    pub hidden: bool,
    /// The folder of the track in the tracks panel.
    pub folder: Option<String>,
    /// The IDs of the picked motifs of a motif track (none for all motifs) and its threshold
    /// of p.
    pub motifs: Option<(Vec<String>, f64)>,
    /// The sample of the lane of B-allele frequencies of a variant track.
    pub baf: Option<String>,
    /// The review marks of the variants of a variant track.
    pub reviews: Reviews,
}

impl SavedTrack {
    pub fn plain(location: Location) -> Self {
        Self {
            location,
            name: None,
            over: None,
            compare: None,
            mode: None,
            hue: Hue::default(),
            height: None,
            options: Options::default(),
            plot: Plot::default(),
            filter: Filter::default(),
            modification: None,
            normalization: None,
            declared: None,
            choice: Choice::default(),
            lift: None,
            hidden: false,
            folder: None,
            motifs: None,
            baf: None,
            reviews: Reviews::default(),
        }
    }
}

/// The keys of the parts of exported images that somebody can turn off.
const SCALE_BAR: &str = "scale bar";
const REGION_BANDS: &str = "region bands";

/// The key of a review mark of a variant of a track.
const REVIEW: &str = "review";

/// The keys of the fields that group, sort and color the samples of a track.
const GROUP_SAMPLES: &str = "group samples";
const SORT_SAMPLES: &str = "sort samples";
const COLOR_SAMPLES: &str = "color samples";

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
                Some(SavedTrack {
                    location: match section.value("made") {
                        Some(key) => Location::Made(key.to_owned()),
                        None => read(section.value("file")?, base),
                    },
                    name: section.value("name").map(str::to_owned),
                    over: section
                        .value("overlay")
                        .and_then(|text| text.parse::<usize>().ok())
                        .and_then(|number| number.checked_sub(1)),
                    compare: comparison(section),
                    mode: section.value("mode").and_then(|name| find(&MODES, name)),
                    hue: section
                        .value("color")
                        .and_then(|name| find(&HUES, name))
                        .unwrap_or_default(),
                    height: section.value("height").and_then(|text| text.parse().ok()),
                    options: options(section),
                    plot: plot(section),
                    filter: Filter {
                        rules: section
                            .entries
                            .iter()
                            .filter_map(|(key, value)| rule(key, value))
                            .collect(),
                        label: section.value("label").map(str::to_owned),
                    },
                    modification: section.value("methylation").map(str::to_owned),
                    normalization: section.value("normalization").map(str::to_owned),
                    declared: section.value("type").map(|kind| Declared {
                        kind: kind.to_owned(),
                        index: section.value("index").map(|text| read(text, base)),
                    }),
                    choice: Choice {
                        group: section.value(GROUP_SAMPLES).map(str::to_owned),
                        sort: section.value(SORT_SAMPLES).map(str::to_owned),
                        color: section.value(COLOR_SAMPLES).map(str::to_owned),
                    },
                    lift: section.value("lift").map(|text| read(text, base)),
                    hidden: section.value("hidden") == Some("on"),
                    folder: section.value("folder").map(str::to_owned),
                    motifs: motif_picks(section),
                    baf: section.value("baf").map(str::to_owned),
                    reviews: Reviews::from_lines(section.values(REVIEW)),
                })
            })
            .collect();
        let hubs = document
            .sections("hub")
            .filter_map(|section| section.value("file").map(|text| read(text, base)))
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
        let samples = document
            .section("samples")
            .and_then(|section| section.value("file"))
            .map(|text| read(text, base));
        let figure = document.section("figure");
        let figure_value = |key| figure.and_then(|section| section.value(key));
        Ok(Self {
            reference: read(reference, base),
            sizes: value("sizes").map(|text| read(text, base)),
            genome: value("genome").map(str::to_owned),
            locus: value("locus").map(str::to_owned),
            reading,
            tracks,
            regions,
            hubs,
            samples,
            figure: Figure {
                title: figure_value("title").unwrap_or_default().to_owned(),
                scale: figure_value(SCALE_BAR) != Some("off"),
                bands: figure_value(REGION_BANDS) != Some("off"),
            },
        })
    }

    pub fn text(&self, base: &Path) -> String {
        let mut session = vec![("reference", written(&self.reference, base))];
        if let Some(sizes) = &self.sizes {
            session.push(("sizes", written(sizes, base)));
        }
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
            sections.push(section("track", track_entries(track, base)));
        }
        for region in &self.regions {
            let entries = vec![
                ("locus", region.locus.clone()),
                ("name", region.name.clone()),
            ];
            sections.push(section("region", entries));
        }
        for hub in &self.hubs {
            sections.push(section("hub", vec![("file", written(hub, base))]));
        }
        if let Some(samples) = &self.samples {
            sections.push(section("samples", vec![("file", written(samples, base))]));
        }
        let mut figure = Vec::new();
        if let Some(title) = self.figure.title() {
            figure.push(("title", title.to_owned()));
        }
        if !self.figure.scale {
            figure.push((SCALE_BAR, "off".to_owned()));
        }
        if !self.figure.bands {
            figure.push((REGION_BANDS, "off".to_owned()));
        }
        if !figure.is_empty() {
            sections.push(section("figure", figure));
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

/// The look of a track. A comparison with another track is not a part of it in a file, because
/// it needs the place of the other track.
fn plot(section: &Section) -> Plot {
    Plot {
        style: if section.value("style") == Some("line") {
            Style::Line
        } else {
            Style::Bars
        },
        linked: section.value("scale") == Some("shared"),
        look: Look {
            pairs: section.value("pairs") != Some("off"),
            mods: section.value("modifications") == Some("on"),
            difference: section.value("methylation difference") == Some("on"),
        },
        colors: section.value("colors") != Some("off"),
        range: section.value("range").and_then(range),
        sashimi: section.value("sashimi") == Some("on"),
        isoforms: section.value("isoforms") == Some("on"),
        junction_reads: section
            .value("junction reads")
            .and_then(|text| text.parse().ok())
            .unwrap_or(1),
        math: None,
        viewpoint: section
            .value("viewpoint")
            .and_then(|text| text.parse::<u64>().ok())
            .and_then(|place| place.checked_sub(1)),
    }
}

/// The entries of the picked motifs of a motif track and of its threshold of p.
fn motif_entries(motifs: Option<&(Vec<String>, f64)>) -> Vec<(&'static str, String)> {
    let Some((ids, p)) = motifs else {
        return Vec::new();
    };
    let picked = if ids.is_empty() {
        "all".to_owned()
    } else {
        ids.join(" ")
    };
    vec![("motifs", picked), ("p", p.to_string())]
}

/// The picked motifs of a motif track and its threshold of p: `motifs = all` for all motifs.
fn motif_picks(section: &Section) -> Option<(Vec<String>, f64)> {
    let ids = section
        .value("motifs")?
        .split_whitespace()
        .filter(|id| *id != "all")
        .map(str::to_owned)
        .collect();
    let p = section
        .value("p")
        .and_then(|text| text.parse().ok())
        .unwrap_or(crate::track::motifs::P);
    Some((ids, p))
}

/// The comparison of a signal track: the place of the other track, counted from 1 in the file,
/// and the operation, by default the log2 ratio.
fn comparison(section: &Section) -> Option<(usize, Operation)> {
    let other = section
        .value("compare")?
        .parse::<usize>()
        .ok()?
        .checked_sub(1)?;
    let operation = section
        .value("math")
        .and_then(|name| {
            Operation::ALL
                .into_iter()
                .find(|operation| operation.label() == name)
        })
        .unwrap_or(Operation::Log2Ratio);
    Some((other, operation))
}

/// The entries of how a track shows its data that differ from the defaults, except the colors
/// of the file.
fn plot_entries(plot: &Plot) -> Vec<(&'static str, String)> {
    let on = || "on".to_owned();
    let mut entries = Vec::new();
    if plot.style == Style::Line {
        entries.push(("style", "line".to_owned()));
    }
    if plot.linked {
        entries.push(("scale", "shared".to_owned()));
    }
    if let Some(range) = plot.range {
        entries.push(("range", format!("{} {}", range.min, range.max)));
    }
    if !plot.look.pairs {
        entries.push(("pairs", "off".to_owned()));
    }
    if plot.look.mods {
        entries.push(("modifications", on()));
    }
    if plot.look.difference {
        entries.push(("methylation difference", on()));
    }
    if plot.sashimi {
        entries.push(("sashimi", on()));
    }
    if plot.isoforms {
        entries.push(("isoforms", on()));
    }
    if plot.junction_reads > 1 {
        entries.push(("junction reads", plot.junction_reads.to_string()));
    }
    if let Some(viewpoint) = plot.viewpoint {
        entries.push(("viewpoint", (viewpoint + 1).to_string()));
    }
    entries
}

/// The entries of the section of a track.
fn track_entries(track: &SavedTrack, base: &Path) -> Vec<(&'static str, String)> {
    let mut entries = match &track.location {
        Location::Made(key) => vec![("made", key.clone())],
        location => vec![("file", written(location, base))],
    };
    if let Some(name) = &track.name {
        entries.push(("name", name.clone()));
    }
    if let Some(over) = track.over {
        entries.push(("overlay", (over + 1).to_string()));
    }
    if let Some((other, operation)) = track.compare {
        entries.push(("compare", (other + 1).to_string()));
        entries.push(("math", operation.label().to_owned()));
    }
    if let Some(mode) = track.mode {
        entries.push(("mode", name(&MODES, mode).to_owned()));
    }
    entries.push(("color", name(&HUES, track.hue).to_owned()));
    if let Some(height) = track.height {
        entries.push(("height", height.to_string()));
    }
    if track.hidden {
        entries.push(("hidden", "on".to_owned()));
    }
    if let Some(folder) = &track.folder {
        entries.push(("folder", folder.clone()));
    }
    entries.extend(motif_entries(track.motifs.as_ref()));
    entries.extend(option_entries(track.options));
    entries.extend(plot_entries(&track.plot));
    if let Some(name) = &track.modification {
        entries.push(("methylation", name.clone()));
    }
    if let Some(name) = &track.baf {
        entries.push(("baf", name.clone()));
    }
    if let Some(name) = &track.normalization {
        entries.push(("normalization", name.clone()));
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
    entries.extend(track.reviews.lines().into_iter().map(|line| (REVIEW, line)));
    if let Some(declared) = &track.declared {
        entries.push(("type", declared.kind.clone()));
        if let Some(index) = &declared.index {
            entries.push(("index", written(index, base)));
        }
    }
    if let Some(lift) = &track.lift {
        entries.push(("lift", written(lift, base)));
    }
    let choice = &track.choice;
    for (key, field) in [
        (GROUP_SAMPLES, &choice.group),
        (SORT_SAMPLES, &choice.sort),
        (COLOR_SAMPLES, &choice.color),
    ] {
        if let Some(field) = field {
            entries.push((key, field.clone()));
        }
    }
    entries
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
        Location::Made(key) => key.clone(),
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

/// The read and methylation options of a track section.
fn options(section: &Section) -> Options {
    let on = |key| section.value(key) == Some("on");
    Options {
        duplicates: on("duplicates"),
        clips: on("clips"),
        downsample: section.value("downsample") != Some("off"),
        group: section
            .value("group")
            .and_then(|name| {
                find(&GROUPS, name)
                    .or_else(|| Some(Grouping::Tag(tag(name.strip_prefix("tag ")?)?)))
            })
            .unwrap_or_default(),
        methyl: Methyl {
            code: 0,
            separate: section.value("strands") == Some("separate"),
        },
        mapq: section
            .value("mapq")
            .and_then(|text| text.parse().ok())
            .unwrap_or(0),
        kinds: Kinds {
            secondary: on("secondary"),
            supplementary: section.value("supplementary") != Some("off"),
            failed: on("failed"),
        },
        color: section.value("colorby").and_then(tag),
        contacts: Showing {
            normalization: None,
            ratio: on("ratio"),
        },
        table: match section.value("column") {
            Some("rows") => Pick::Rows,
            Some(text) => text
                .parse::<u16>()
                .ok()
                .and_then(|column| column.checked_sub(1))
                .map_or(Pick::Auto, Pick::Column),
            None => Pick::Auto,
        },
        lift: 0,
        mates: on("mates"),
        baf: None,
    }
}

/// The entries of the read and methylation options that differ from the defaults.
fn option_entries(options: Options) -> Vec<(&'static str, String)> {
    let on = || "on".to_owned();
    let mut entries = Vec::new();
    if options.duplicates {
        entries.push(("duplicates", on()));
    }
    if options.mates {
        entries.push(("mates", on()));
    }
    if options.clips {
        entries.push(("clips", on()));
    }
    if !options.downsample {
        entries.push(("downsample", "off".to_owned()));
    }
    match options.group {
        Grouping::None => {}
        Grouping::Tag(tag) => entries.push(("group", format!("tag {}", tag_name(tag)))),
        group => entries.push(("group", name(&GROUPS, group).to_owned())),
    }
    if let Some(tag) = options.color {
        entries.push(("colorby", tag_name(tag)));
    }
    if options.contacts.ratio {
        entries.push(("ratio", on()));
    }
    match options.table {
        Pick::Auto => {}
        Pick::Column(column) => entries.push(("column", (u32::from(column) + 1).to_string())),
        Pick::Rows => entries.push(("column", "rows".to_owned())),
    }
    if options.methyl.separate {
        entries.push(("strands", "separate".to_owned()));
    }
    if options.mapq > 0 {
        entries.push(("mapq", options.mapq.to_string()));
    }
    if options.kinds.secondary {
        entries.push(("secondary", on()));
    }
    if !options.kinds.supplementary {
        entries.push(("supplementary", "off".to_owned()));
    }
    if options.kinds.failed {
        entries.push(("failed", on()));
    }
    entries
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

    use genie_core::samples::Choice;

    use super::{Saved, SavedRegion, SavedTrack, is_session};
    use crate::export::Figure;
    use crate::location::Location;
    use crate::reading::{Frames, Reading};
    use crate::track::{Declared, Filter, Grouping, Kinds, Methyl, Options, Pick, Plot, Showing};

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

    fn ratios() -> Options {
        Options {
            contacts: Showing {
                normalization: None,
                ratio: true,
            },
            table: Pick::Column(5),
            ..Options::default()
        }
    }

    fn read_options() -> Options {
        Options {
            duplicates: true,
            clips: true,
            downsample: false,
            group: Grouping::Phase,
            methyl: Methyl {
                code: 0,
                separate: true,
            },
            mapq: 20,
            kinds: Kinds {
                secondary: true,
                supplementary: false,
                failed: true,
            },
            color: Some(*b"NM"),
            contacts: Showing::default(),
            table: Pick::Rows,
            lift: 0,
            mates: true,
            baf: None,
        }
    }

    #[test]
    fn read_tracks_keep_their_tag_group_and_tag_colors() {
        let text = saved().text(&base());
        assert!(text.contains("colorby = NM"), "{text}");
        let mut tagged = saved();
        tagged.tracks[0].options.group = Grouping::Tag(*b"CB");
        let text = tagged.text(&base());
        assert!(text.contains("group = tag CB"), "{text}");
        assert_eq!(Saved::parse(&text, &base()), Ok(tagged));
    }

    fn saved() -> Saved {
        let chains = Location::File(base().join("chains").join("hg19ToHg38.over.chain.gz"));
        Saved {
            reference: Location::File(base().join("data").join("ref.fa.gz")),
            sizes: None,
            genome: None,
            locus: Some("chr1:51,971-52,030".to_owned()),
            reading: Reading {
                reverse: true,
                frames: Frames::Six,
            },
            tracks: vec![
                SavedTrack {
                    name: Some("Tumor A".to_owned()),
                    mode: Some(Mode::Squished),
                    hue: Hue::Blue,
                    height: Some(90.0),
                    options: read_options(),
                    plot: Plot {
                        look: Look {
                            mods: true,
                            difference: true,
                            ..Look::default()
                        },
                        ..Plot::default()
                    },
                    modification: Some("5hmC".to_owned()),
                    declared: Some(Declared {
                        kind: "bam".to_owned(),
                        index: Some(Location::File(base().join("index").join("reads.bai"))),
                    }),
                    ..SavedTrack::plain(Location::File(base().join("reads.bam")))
                },
                SavedTrack {
                    hue: Hue::Vermillion,
                    options: ratios(),
                    filter: Filter {
                        rules: vec![
                            rule("Clinical significance", Test::Keep("Pathogenic".to_owned())),
                            rule("Minor allele frequency", Test::AtLeast(0.05)),
                            rule("Gene type", Test::Hide("lncRNA".to_owned())),
                            rule("Quality", Test::AtMost(30.0)),
                        ],
                        label: Some("Gene symbol".to_owned()),
                    },
                    normalization: Some("KR".to_owned()),
                    choice: Choice {
                        group: Some("super_pop".to_owned()),
                        sort: Some("age".to_owned()),
                        color: Some("sex".to_owned()),
                    },
                    lift: Some(chains),
                    hidden: true,
                    ..SavedTrack::plain(Location::File(PathBuf::from("/elsewhere/genes.bed.gz")))
                },
                SavedTrack {
                    over: Some(0),
                    hue: Hue::Green,
                    height: Some(64.0),
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
                    declared: Some(Declared {
                        kind: "bigWig".to_owned(),
                        index: None,
                    }),
                    folder: Some("Signals".to_owned()),
                    ..SavedTrack::plain(Location::File(base().join("coverage.bw")))
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
            hubs: vec![
                Location::File(base().join("hubs").join("hub.txt")),
                Location::parse("https://example.org/hub.txt").unwrap(),
            ],
            samples: Some(Location::File(base().join("groups.tsv"))),
            figure: Figure::default(),
        }
    }

    #[test]
    fn sessions_keep_the_title_and_the_parts_of_figures() {
        let text = saved().text(&base());
        assert!(!text.contains("[figure]"), "{text}");
        let mut titled = saved();
        titled.figure = Figure {
            title: "TP53 promoter".to_owned(),
            scale: false,
            bands: false,
        };
        let text = titled.text(&base());
        for line in [
            "[figure]",
            "title = TP53 promoter",
            "scale bar = off",
            "region bands = off",
        ] {
            assert!(text.contains(line), "{line}\n{text}");
        }
        assert_eq!(Saved::parse(&text, &base()), Ok(titled));
    }

    #[test]
    fn sessions_keep_the_sample_table_and_the_sample_fields_of_tracks() {
        let text = saved().text(&base());
        for line in [
            "[samples]",
            "file = groups.tsv",
            "group samples = super_pop",
            "sort samples = age",
            "color samples = sex",
        ] {
            assert!(text.contains(line), "{line}\n{text}");
        }
        assert_eq!(Saved::parse(&text, &base()), Ok(saved()));
        let mut plain = saved();
        plain.samples = None;
        plain.tracks[1].choice = Choice::default();
        let text = plain.text(&base());
        assert!(!text.contains("samples"), "{text}");
        assert_eq!(Saved::parse(&text, &base()), Ok(plain));
    }

    #[test]
    fn hub_tracks_keep_their_type_and_sessions_keep_their_hubs() {
        let text = saved().text(&base());
        for line in [
            "type = bam",
            "index = index/reads.bai",
            "type = bigWig",
            "file = hubs/hub.txt",
            "file = https://example.org/hub.txt",
        ] {
            assert!(text.contains(line), "{line}\n{text}");
        }
        assert_eq!(text.matches("[hub]").count(), 2, "{text}");
        assert_eq!(text.matches("index =").count(), 1, "{text}");
    }

    #[test]
    fn read_tracks_keep_their_sashimi_plots() {
        let mut sashimi = saved();
        sashimi.tracks[0].plot.sashimi = true;
        sashimi.tracks[0].plot.junction_reads = 5;
        let text = sashimi.text(&base());
        assert!(text.contains("sashimi = on"), "{text}");
        assert!(text.contains("junction reads = 5"), "{text}");
        assert_eq!(Saved::parse(&text, &base()), Ok(sashimi));
        assert!(!saved().text(&base()).contains("sashimi"));
    }

    #[test]
    fn read_tracks_keep_their_isoforms() {
        let mut isoforms = saved();
        isoforms.tracks[0].plot.isoforms = true;
        let text = isoforms.text(&base());
        assert!(text.contains("isoforms = on"), "{text}");
        assert_eq!(Saved::parse(&text, &base()), Ok(isoforms));
        assert!(!saved().text(&base()).contains("isoforms"));
    }

    #[test]
    fn motif_tracks_keep_their_motifs_and_their_p() {
        let mut motifs = saved();
        motifs.tracks[0].motifs = Some((vec!["MA0139.1".to_owned(), "MA0102.4".to_owned()], 0.001));
        motifs.tracks[1].motifs = Some((Vec::new(), 0.0001));
        let text = motifs.text(&base());
        assert!(text.contains("motifs = MA0139.1 MA0102.4"), "{text}");
        assert!(text.contains("p = 0.001\n"), "{text}");
        assert!(text.contains("motifs = all"), "{text}");
        assert_eq!(Saved::parse(&text, &base()), Ok(motifs));
        assert!(!saved().text(&base()).contains("motifs ="));
    }

    #[test]
    fn variant_tracks_keep_their_review_marks() {
        use genie_core::review::{Key, Review, Verdict};
        let mut reviewed = saved();
        let key = Key {
            contig: "chr1".to_owned(),
            position: 20,
            reference: "A".to_owned(),
            alternates: "G".to_owned(),
        };
        let review = Review {
            verdict: Verdict::Unsure,
            id: "rs21".to_owned(),
            note: "low MAPQ; look at = signs".to_owned(),
        };
        reviewed.tracks[1].reviews.set(key, Some(review));
        let text = reviewed.text(&base());
        let line = "review = unsure chr1 21 A G rs21 low MAPQ; look at = signs";
        assert!(text.contains(line), "{text}");
        assert_eq!(Saved::parse(&text, &base()), Ok(reviewed));
        assert!(!saved().text(&base()).contains("review ="));
    }

    #[test]
    fn tracks_made_from_the_reference_keep_what_they_show() {
        let mut made = saved();
        made.tracks[0].location = Location::Made("sites EcoRI G^AATTC".to_owned());
        made.tracks[1].location = Location::Made("gc 100".to_owned());
        let text = made.text(&base());
        assert!(text.contains("made = sites EcoRI G^AATTC"), "{text}");
        assert!(text.contains("made = gc 100"), "{text}");
        assert!(!text.contains("file = gc 100"), "{text}");
        assert_eq!(Saved::parse(&text, &base()), Ok(made));
    }

    #[test]
    fn lifted_tracks_keep_their_chain_file() {
        let text = saved().text(&base());
        assert!(
            text.contains("lift = chains/hg19ToHg38.over.chain.gz"),
            "{text}"
        );
        assert_eq!(text.matches("lift =").count(), 1, "{text}");
        assert_eq!(text.matches("hidden = on").count(), 1, "{text}");
        assert_eq!(text.matches("folder = Signals").count(), 1, "{text}");
        assert_eq!(Saved::parse(&text, &base()), Ok(saved()));
    }

    #[test]
    fn sessions_keep_addresses_as_written() {
        let mut saved = saved();
        saved.reference = Location::parse("https://example.org/hg38.2bit").unwrap();
        saved.sizes = Location::parse("https://example.org/hg38.chrom.sizes.txt").ok();
        saved.tracks[0].location = Location::parse("https://example.org/a.bam?k=1").unwrap();
        let text = saved.text(&base());
        assert!(
            text.contains("reference = https://example.org/hg38.2bit"),
            "{text}"
        );
        assert!(
            text.contains("sizes = https://example.org/hg38.chrom.sizes.txt"),
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
    fn read_tracks_keep_their_quick_filters() {
        let text = saved().text(&base());
        for line in [
            "mapq = 20",
            "secondary = on",
            "supplementary = off",
            "failed = on",
        ] {
            assert!(text.contains(line), "{line}\n{text}");
        }
        assert_eq!(text.matches("mapq =").count(), 1, "{text}");
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
