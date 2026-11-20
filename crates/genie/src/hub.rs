//! UCSC track hubs: genie reads the hub, its genomes and the tracks of the genome that matches
//! the open reference.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;

use genie_formats::http;
use genie_formats::hub::{self as text, HubFile};

use crate::location::Location;
use crate::track::Typed;

/// The largest hub, genomes or trackDb file that genie reads.
pub const TEXT_LIMIT: usize = 16 << 20;
/// How many levels of `include` lines genie follows.
const INCLUDES: usize = 3;
/// The deepest level that genie shows: super track, composite track, view and track.
const DEPTH: usize = 3;

/// A track hub: its labels, its genomes, and the tracks of the genome that genie chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hub {
    pub location: Location,
    pub short: String,
    pub long: String,
    pub genomes: Vec<Genome>,
    /// The genome whose tracks `tracks` holds.
    pub chosen: Option<usize>,
    pub tracks: Vec<Entry>,
}

impl Hub {
    pub fn genome(&self) -> Option<&Genome> {
        self.genomes.get(self.chosen?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Genome {
    pub name: String,
    pub organism: String,
    pub description: String,
    /// The 2bit sequence of an assembly hub genome.
    pub sequence: Option<Location>,
    /// The file with the length of each contig of the sequence.
    pub sizes: Option<Location>,
    pub position: Option<String>,
    track_db: Option<Location>,
}

/// A track of a hub, or a container that holds tracks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub short: String,
    pub long: String,
    pub kind: String,
    pub data: Option<Location>,
    pub index: Option<Location>,
    /// The short label of the container that holds the track.
    pub group: Option<String>,
    pub depth: usize,
    pub container: bool,
    words: String,
}

impl Entry {
    pub fn typed(&self) -> Option<Typed> {
        Typed::of(&self.kind)
    }

    /// True when the labels, the name or the type hold each word, in any case.
    pub fn matches(&self, words: &[String]) -> bool {
        words.iter().all(|word| self.words.contains(word.as_str()))
    }
}

/// What the open session holds, so that genie can choose the genome of a hub.
#[derive(Debug, Clone, Copy, Default)]
pub struct Wanted<'a> {
    pub genome: Option<&'a str>,
    pub reference: Option<&'a Location>,
}

impl Wanted<'_> {
    fn choose(&self, genomes: &[Genome]) -> Option<usize> {
        let reference = self.reference?;
        let named = |name: &str| genomes.iter().position(|genome| genome.name == name);
        let file = reference.name();
        let stem = file.split('.').next().unwrap_or_default();
        genomes
            .iter()
            .position(|genome| genome.sequence.as_ref() == Some(reference))
            .or_else(|| self.genome.and_then(named))
            .or_else(|| named(stem).filter(|_| !stem.is_empty()))
    }
}

/// Reads a hub, its genomes, and the tracks of the genome that matches the session.
pub fn load(location: &Location, wanted: Wanted<'_>) -> Result<Hub, String> {
    let name = location.name();
    let HubFile {
        hub,
        genomes,
        tracks,
    } = text::hub(&read_text(location)?).ok_or_else(|| {
        format!("The file {name} is not a track hub. A hub file starts with a hub line.")
    })?;
    let short = if hub.short.is_empty() {
        hub.name.clone()
    } else {
        hub.short.clone()
    };
    let (genomes, base, held) = if hub.one_file {
        (genomes, location.clone(), tracks)
    } else {
        let file = hub
            .genomes
            .as_deref()
            .ok_or_else(|| format!("The hub {short} has no genomesFile line."))?;
        let at = location.join(file)?;
        (text::genomes(&read_text(&at)?), at, Vec::new())
    };
    let genomes: Vec<Genome> = genomes.iter().map(|genome| place(genome, &base)).collect();
    if genomes.is_empty() {
        return Err(format!("The hub {short} has no genomes."));
    }
    let chosen = wanted.choose(&genomes);
    let tracks = match chosen.map(|index| &genomes[index]) {
        Some(genome) if hub.one_file => {
            let found: Vec<text::Track> = held
                .into_iter()
                .filter(|(owner, _)| *owner == genome.name)
                .map(|(_, track)| track)
                .collect();
            arrange(&found, location)
        }
        Some(Genome {
            track_db: Some(file),
            ..
        }) => arrange(&track_db(file)?, file),
        _ => Vec::new(),
    };
    Ok(Hub {
        location: location.clone(),
        long: if hub.long.is_empty() {
            short.clone()
        } else {
            hub.long
        },
        short,
        genomes,
        chosen,
        tracks,
    })
}

fn place(genome: &text::Genome, base: &Location) -> Genome {
    let resolve = |text: &Option<String>| text.as_deref().and_then(|text| base.join(text).ok());
    Genome {
        name: genome.name.clone(),
        organism: genome.organism.clone(),
        description: genome.description.clone(),
        sequence: resolve(&genome.two_bit),
        sizes: resolve(&genome.sizes),
        position: genome.position.clone(),
        track_db: resolve(&genome.track_db),
    }
}

/// The text of a small hub file on this computer or on the web.
pub fn read_text(location: &Location) -> Result<String, String> {
    read_within(location, TEXT_LIMIT)
}

fn read_within(location: &Location, limit: usize) -> Result<String, String> {
    let name = location.name();
    let large = || {
        format!(
            "The hub file {name} is larger than {} MB.",
            limit.div_ceil(1 << 20)
        )
    };
    let bytes = match location {
        Location::File(path) => {
            let cannot = |error: io::Error| format!("genie cannot open {name}. {error}");
            let size = fs::metadata(path).map_err(cannot)?.len();
            if size > limit as u64 {
                return Err(large());
            }
            fs::read(path).map_err(cannot)?
        }
        Location::Url(url) => {
            let mut bytes = Vec::new();
            let mut over = false;
            let result = http::download(
                url,
                0,
                |part| {
                    if bytes.len() + part.len() > limit {
                        over = true;
                        return Err(io::Error::other("The file is too large."));
                    }
                    bytes.extend_from_slice(part);
                    Ok(())
                },
                |_, _| {},
            );
            if over {
                return Err(large());
            }
            result.map_err(|error| error.to_string())?;
            bytes
        }
        Location::Made(_) => return location.open().map(|_| String::new()),
    };
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The tracks of a trackDb file and of the files that its `include` lines name.
fn track_db(location: &Location) -> Result<Vec<text::Track>, String> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut files = vec![(location.clone(), 0)];
    while let Some((file, depth)) = files.pop() {
        if !seen.insert(file.clone()) {
            continue;
        }
        let (tracks, includes) = text::tracks(&read_text(&file)?);
        found.extend(tracks);
        if depth < INCLUDES {
            for include in includes.iter().rev() {
                files.push((file.join(include)?, depth + 1));
            }
        }
    }
    Ok(found)
}

/// The tracks in the order of the tree of containers, with addresses relative to `base`.
fn arrange(tracks: &[text::Track], base: &Location) -> Vec<Entry> {
    let count = tracks.len();
    let mut names = HashMap::new();
    for (index, track) in tracks.iter().enumerate() {
        names.entry(track.name.as_str()).or_insert(index);
    }
    let parents: Vec<Option<usize>> = tracks
        .iter()
        .enumerate()
        .map(|(index, track)| {
            let parent = track.parent.as_deref()?;
            names.get(parent).copied().filter(|found| *found != index)
        })
        .collect();
    let mut children = vec![Vec::new(); count];
    let mut roots = Vec::new();
    for (index, parent) in parents.iter().enumerate() {
        match parent {
            Some(parent) => children[*parent].push(index),
            None => roots.push(index),
        }
    }
    let mut order = Vec::with_capacity(count);
    let mut placed = vec![false; count];
    let mut stack: Vec<(usize, usize)> = roots.iter().rev().map(|root| (*root, 0)).collect();
    loop {
        while let Some((index, depth)) = stack.pop() {
            if std::mem::replace(&mut placed[index], true) {
                continue;
            }
            order.push((index, depth));
            let below = (depth + 1).min(DEPTH);
            stack.extend(children[index].iter().rev().map(|child| (*child, below)));
        }
        match placed.iter().position(|done| !done) {
            Some(index) => stack.push((index, 0)),
            None => break,
        }
    }
    let kind_of = |mut index: usize| {
        for _ in 0..=count {
            if !tracks[index].kind.is_empty() {
                return tracks[index].kind.clone();
            }
            match parents[index] {
                Some(parent) => index = parent,
                None => break,
            }
        }
        String::new()
    };
    let resolve = |text: &Option<String>| text.as_deref().and_then(|text| base.join(text).ok());
    order
        .into_iter()
        .map(|(index, depth)| {
            let track = &tracks[index];
            let data = resolve(&track.data);
            let kind = kind_of(index);
            let words = format!("{} {} {} {kind}", track.short, track.long, track.name);
            Entry {
                name: track.name.clone(),
                short: track.short.clone(),
                long: track.long.clone(),
                data: data.clone(),
                index: resolve(&track.index),
                group: parents[index].map(|parent| tracks[parent].short.clone()),
                depth,
                container: track.container || (data.is_none() && !children[index].is_empty()),
                words: words.to_lowercase(),
                kind,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{Entry, Wanted, load, read_within};
    use crate::location::Location;
    use crate::scratch::Scratch;
    use crate::track::Typed;

    const HUB: &str = "hub mine\nshortLabel My hub\nlongLabel My long hub\n\
        genomesFile genomes.txt\nemail a@b.c\n";
    const GENOMES: &str = "genome hg38\ntrackDb hg38/trackDb.txt\n\n\
        genome mm10\ntrackDb mm10/trackDb.txt\n";
    const TRACK_DB: &str = "track wave\nsuperTrack signals show\ntype bigWig 0 10\n\
        bigDataUrl wave.bw\nshortLabel Wave\n\n\
        track signals\nsuperTrack on show\nshortLabel Signals\n\n\
        track peaks\ncompositeTrack on\nparent signals\nshortLabel Peaks\ntype bigBed 6\n\n\
        track peaksView\nview PK\nparent peaks\nshortLabel Peak view\n\n\
        track peak1\nparent peaksView on\ntype bigNarrowPeak\nbigDataUrl ../data/peak1.bb\n\
        shortLabel Peak 1\nlongLabel First peaks of the cells\n\n\
        track peak2\nparent peaksView off\nbigDataUrl ../data/peak2.bb\nshortLabel Peak 2\n\n\
        include more.txt\n";
    const MORE: &str = "track reads\ntype bam\nbigDataUrl reads.bam\n\
        bigDataIndex index/reads.bai\nshortLabel Reads\n\n\
        track contacts\ntype hic\nbigDataUrl contacts.hic\nshortLabel Contacts\n";

    fn folder(scratch: &Scratch) -> PathBuf {
        let folder = scratch.file("hub.txt", HUB).parent().unwrap().to_owned();
        for genome in ["hg38", "mm10"] {
            fs::create_dir_all(folder.join(genome)).unwrap();
        }
        fs::write(folder.join("genomes.txt"), GENOMES).unwrap();
        fs::write(folder.join("hg38/trackDb.txt"), TRACK_DB).unwrap();
        fs::write(folder.join("hg38/more.txt"), MORE).unwrap();
        fs::write(folder.join("mm10/trackDb.txt"), "").unwrap();
        folder
    }

    fn file(path: &Path) -> Location {
        Location::File(path.to_owned())
    }

    fn shown(tracks: &[Entry]) -> Vec<(&str, usize, bool)> {
        tracks
            .iter()
            .map(|entry| (entry.name.as_str(), entry.depth, entry.container))
            .collect()
    }

    #[test]
    fn hubs_on_this_computer_show_the_tracks_of_the_open_genome() {
        let scratch = Scratch::new("hub-local");
        let folder = folder(&scratch);
        let reference = Location::File(PathBuf::from("/data/genome.2bit"));
        let wanted = Wanted {
            genome: Some("hg38"),
            reference: Some(&reference),
        };
        let hub = load(&file(&folder.join("hub.txt")), wanted).unwrap();
        assert_eq!(
            (hub.short.as_str(), hub.long.as_str()),
            ("My hub", "My long hub")
        );
        let names: Vec<&str> = hub
            .genomes
            .iter()
            .map(|genome| genome.name.as_str())
            .collect();
        assert_eq!(names, ["hg38", "mm10"]);
        assert_eq!(hub.genome().unwrap().name, "hg38");
        assert_eq!(
            shown(&hub.tracks),
            [
                ("signals", 0, true),
                ("wave", 1, false),
                ("peaks", 1, true),
                ("peaksView", 2, true),
                ("peak1", 3, false),
                ("peak2", 3, false),
                ("reads", 0, false),
                ("contacts", 0, false),
            ]
        );
        let peak = &hub.tracks[4];
        assert_eq!(peak.data, Some(file(&folder.join("data/peak1.bb"))));
        assert_eq!(peak.group.as_deref(), Some("Peak view"));
        assert_eq!(peak.long, "First peaks of the cells");
        assert_eq!(peak.typed(), Some(Typed::Big));
        assert_eq!(hub.tracks[5].kind, "bigBed");
        let reads = &hub.tracks[6];
        assert_eq!(reads.data, Some(file(&folder.join("hg38/reads.bam"))));
        assert_eq!(
            reads.index,
            Some(file(&folder.join("hg38/index/reads.bai")))
        );
        assert_eq!(reads.typed(), Some(Typed::Bam));
        assert_eq!(hub.tracks[7].typed(), Some(Typed::Hic));
        let words = |text: &str| -> Vec<String> {
            text.split_whitespace().map(str::to_lowercase).collect()
        };
        assert!(peak.matches(&words("FIRST cells")));
        assert!(peak.matches(&words("narrowpeak")));
        assert!(!peak.matches(&words("first reads")));
    }

    #[test]
    fn genomes_match_by_sequence_name_or_file_name() {
        let scratch = Scratch::new("hub-match");
        let folder = folder(&scratch);
        let hub = file(&folder.join("hub.txt"));
        let fasta = Location::File(PathBuf::from("/data/mm10.fa.gz"));
        let by_file = Wanted {
            genome: None,
            reference: Some(&fasta),
        };
        let found = load(&hub, by_file).unwrap();
        assert_eq!(found.genome().unwrap().name, "mm10");
        assert_eq!(found.tracks.len(), 0);
        let other = Location::File(PathBuf::from("/data/genome.fa"));
        let none = Wanted {
            genome: Some("dm6"),
            reference: Some(&other),
        };
        let found = load(&hub, none).unwrap();
        assert_eq!((found.chosen, found.tracks.len()), (None, 0));
    }

    #[test]
    fn assembly_hubs_in_one_file_name_their_sequence() {
        let text = "hub single\nshortLabel One\nuseOneFile on\nemail a@b.c\n\n\
            genome myAsm\ntwoBitPath seq/myAsm.2bit\nchromSizes seq/myAsm.chrom.sizes.txt\n\
            organism Fly\ndescription My assembly\ndefaultPos chr1:1-1000\n\n\
            track genes\ntype bigGenePred\nbigDataUrl genes.bb\nshortLabel Genes\n";
        let scratch = Scratch::new("hub-assembly");
        let hub = file(&scratch.file("hub.txt", text));
        let other = Location::File(PathBuf::from("/data/genome.fa"));
        let wanted = |reference| Wanted {
            genome: None,
            reference: Some(reference),
        };
        let found = load(&hub, wanted(&other)).unwrap();
        assert_eq!(found.long, "One");
        assert_eq!(found.chosen, None);
        let genome = &found.genomes[0];
        let sequence = hub.join("seq/myAsm.2bit").unwrap();
        assert_eq!(genome.sequence.as_ref(), Some(&sequence));
        assert_eq!(genome.sizes, hub.join("seq/myAsm.chrom.sizes.txt").ok());
        assert_eq!(
            (genome.organism.as_str(), genome.description.as_str()),
            ("Fly", "My assembly")
        );
        assert_eq!(genome.position.as_deref(), Some("chr1:1-1000"));
        let found = load(&hub, wanted(&sequence)).unwrap();
        assert_eq!(found.chosen, Some(0));
        assert_eq!(found.tracks[0].data, hub.join("genes.bb").ok());
    }

    #[test]
    fn hubs_on_the_web_join_addresses_to_their_files() {
        let track_db = "track a\ntype bigBed\nbigDataUrl ../files/a.bb\nshortLabel A\n\n\
            track b\ntype bigWig\nbigDataUrl https://example.org/b.bw\nshortLabel B\n";
        let files = [
            ("/hubs/x/hub.txt", HUB),
            ("/hubs/x/genomes.txt", GENOMES),
            ("/hubs/x/hg38/trackDb.txt", track_db),
        ]
        .map(|(path, text)| (path.to_owned(), text.as_bytes().to_vec()))
        .to_vec();
        let server = crate::server::serve(files);
        let hub = Location::parse(&server.url("/hubs/x/hub.txt")).unwrap();
        let reference = Location::File(PathBuf::from("/data/hg38.fa"));
        let wanted = Wanted {
            genome: None,
            reference: Some(&reference),
        };
        let found = load(&hub, wanted).unwrap();
        let data: Vec<String> = found
            .tracks
            .iter()
            .filter_map(|entry| entry.data.as_ref().map(ToString::to_string))
            .collect();
        assert_eq!(
            data,
            [
                server.url("/hubs/x/files/a.bb"),
                "https://example.org/b.bw".to_owned()
            ]
        );
    }

    #[test]
    fn broken_hubs_say_why() {
        let scratch = Scratch::new("hub-broken");
        let reference = Location::File(PathBuf::from("/data/hg38.fa"));
        let wanted = Wanted {
            genome: None,
            reference: Some(&reference),
        };
        let open =
            |name: &str, text: &str| load(&file(&scratch.file(name, text)), wanted).unwrap_err();
        assert_eq!(
            open("tracks.txt", "track a\ntype bigBed\n"),
            "The file tracks.txt is not a track hub. A hub file starts with a hub line."
        );
        assert_eq!(
            open("bare.txt", "hub bare\nshortLabel Bare\n"),
            "The hub Bare has no genomesFile line."
        );
        let missing = open("lost.txt", HUB);
        assert!(
            missing.starts_with("genie cannot open genomes.txt."),
            "{missing}"
        );
        let empty = open("empty.txt", "hub empty\nuseOneFile on\n");
        assert_eq!(empty, "The hub empty has no genomes.");
        let large = scratch.file("large.txt", "x".repeat(3 << 20));
        assert_eq!(
            read_within(&file(&large), 2 << 20).unwrap_err(),
            "The hub file large.txt is larger than 2 MB."
        );
    }
}
