use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use genie_core::samples::Choice;
use genie_formats::fields::{self, Rule, Test};
use genie_formats::http::Url;
use genie_formats::ini::{self, Document, Section};

use genie_render::alignments::Look;
use genie_render::features::Mode;
use genie_render::scene::Hue;
use genie_render::signal::{Scale, Style};

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
    pub genome: Option<String>,
    pub locus: Option<String>,
    pub reading: Reading,
    pub tracks: Vec<SavedTrack>,
    pub regions: Vec<SavedRegion>,
    /// The track hubs that the session shows in the Tracks menu.
    pub hubs: Vec<Location>,
    /// The sample table of the session.
    pub samples: Option<Location>,
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
    pub modification: Option<String>,
    /// The normalization that somebody chose for a Hi-C map.
    pub normalization: Option<String>,
    pub declared: Option<Declared>,
    /// The fields of the sample table that group, sort and color the samples of the track.
    pub choice: Choice,
    /// The chain file that moves the data of the track to the assembly in view.
    pub lift: Option<Location>,
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
            modification: None,
            normalization: None,
            declared: None,
            choice: Choice::default(),
            lift: None,
        }
    }
}

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
                    options: options(section),
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
                        sashimi: section.value("sashimi") == Some("on"),
                        junction_reads: section
                            .value("junction reads")
                            .and_then(|text| text.parse().ok())
                            .unwrap_or(1),
                    },
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
        Ok(Self {
            reference: read(reference, base),
            genome: value("genome").map(str::to_owned),
            locus: value("locus").map(str::to_owned),
            reading,
            tracks,
            regions,
            hubs,
            samples,
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

/// The entries of the section of a track.
fn track_entries(track: &SavedTrack, base: &Path) -> Vec<(&'static str, String)> {
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
    entries.extend(option_entries(track.options));
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
    if track.plot.sashimi {
        entries.push(("sashimi", "on".to_owned()));
    }
    if track.plot.junction_reads > 1 {
        entries.push(("junction reads", track.plot.junction_reads.to_string()));
    }
    if let Some(name) = &track.modification {
        entries.push(("methylation", name.clone()));
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
    }
}

/// The entries of the read and methylation options that differ from the defaults.
fn option_entries(options: Options) -> Vec<(&'static str, String)> {
    let on = || "on".to_owned();
    let mut entries = Vec::new();
    if options.duplicates {
        entries.push(("duplicates", on()));
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
                    options: read_options(),
                    plot: Plot {
                        look: Look {
                            pairs: false,
                            mods: true,
                        },
                        ..Plot::default()
                    },
                    filter: Filter::default(),
                    modification: Some("5hmC".to_owned()),
                    normalization: None,
                    declared: Some(Declared {
                        kind: "bam".to_owned(),
                        index: Some(Location::File(base().join("index").join("reads.bai"))),
                    }),
                    choice: Choice::default(),
                    lift: None,
                },
                SavedTrack {
                    location: Location::File(PathBuf::from("/elsewhere/genes.bed.gz")),
                    name: None,
                    over: None,
                    mode: None,
                    hue: Hue::Vermillion,
                    height: None,
                    options: ratios(),
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
                    modification: None,
                    normalization: Some("KR".to_owned()),
                    declared: None,
                    choice: Choice {
                        group: Some("super_pop".to_owned()),
                        sort: Some("age".to_owned()),
                        color: Some("sex".to_owned()),
                    },
                    lift: Some(chains),
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
        }
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
    fn lifted_tracks_keep_their_chain_file() {
        let text = saved().text(&base());
        assert!(
            text.contains("lift = chains/hg19ToHg38.over.chain.gz"),
            "{text}"
        );
        assert_eq!(text.matches("lift =").count(), 1, "{text}");
        assert_eq!(Saved::parse(&text, &base()), Ok(saved()));
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
