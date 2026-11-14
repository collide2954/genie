//! UCSC track hubs: the stanzas of hub.txt, genomes.txt and trackDb.txt.

/// One block of "key value" lines; a blank line ends it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stanza {
    pub fields: Vec<(String, String)>,
}

impl Stanza {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    fn first(&self) -> Option<&str> {
        self.fields.first().map(|(name, _)| name.as_str())
    }
}

/// The stanzas of a hub file, and the files that its `include` lines name.
pub fn stanzas(text: &str) -> (Vec<Stanza>, Vec<String>) {
    let mut found = Vec::new();
    let mut included = Vec::new();
    let mut current = Stanza::default();
    let mut pending = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if pending.is_empty() && line.starts_with('#') {
            continue;
        }
        if let Some(stem) = line.strip_suffix('\\') {
            pending.push_str(stem.trim_end());
            pending.push(' ');
            continue;
        }
        let line = if pending.is_empty() {
            line.to_owned()
        } else {
            let mut joined = std::mem::take(&mut pending);
            joined.push_str(line);
            joined
        };
        if line.is_empty() {
            if !current.fields.is_empty() {
                found.push(std::mem::take(&mut current));
            }
            continue;
        }
        let (key, value) = match line.split_once(char::is_whitespace) {
            Some((key, value)) => (key, value.trim()),
            None => (line.as_str(), ""),
        };
        if key == "include" && current.fields.is_empty() {
            included.push(value.to_owned());
            continue;
        }
        current.fields.push((key.to_owned(), value.to_owned()));
    }
    if !current.fields.is_empty() {
        found.push(current);
    }
    (found, included)
}

fn word(value: &str) -> &str {
    value.split_whitespace().next().unwrap_or_default()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hub {
    pub name: String,
    pub short: String,
    pub long: String,
    pub genomes: Option<String>,
    pub one_file: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Genome {
    pub name: String,
    pub track_db: Option<String>,
    pub two_bit: Option<String>,
    pub organism: String,
    pub description: String,
    pub position: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Track {
    pub name: String,
    pub short: String,
    pub long: String,
    pub kind: String,
    pub data: Option<String>,
    pub index: Option<String>,
    pub parent: Option<String>,
    pub container: bool,
}

/// A hub.txt file: the hub, and for a hub in one file (`useOneFile on`) its genomes and the
/// tracks of each genome by genome name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HubFile {
    pub hub: Hub,
    pub genomes: Vec<Genome>,
    pub tracks: Vec<(String, Track)>,
}

pub fn hub(text: &str) -> Option<HubFile> {
    let (found, _) = stanzas(text);
    let first = found
        .first()
        .filter(|stanza| stanza.first() == Some("hub"))?;
    let short = first.get("shortLabel").unwrap_or_default();
    let hub = Hub {
        name: word(first.get("hub")?).to_owned(),
        short: short.to_owned(),
        long: first.get("longLabel").unwrap_or(short).to_owned(),
        genomes: first.get("genomesFile").map(str::to_owned),
        one_file: first
            .get("useOneFile")
            .is_some_and(|value| word(value) == "on"),
    };
    let mut genomes = Vec::new();
    let mut tracks = Vec::new();
    if hub.one_file {
        let mut current: Option<String> = None;
        for stanza in &found[1..] {
            match stanza.first() {
                Some("genome") => {
                    let genome = genome(stanza);
                    current = Some(genome.name.clone());
                    genomes.push(genome);
                }
                Some("track") => {
                    if let Some(name) = &current {
                        tracks.push((name.clone(), track(stanza)));
                    }
                }
                _ => {}
            }
        }
    }
    Some(HubFile {
        hub,
        genomes,
        tracks,
    })
}

fn genome(stanza: &Stanza) -> Genome {
    let text = |key: &str| stanza.get(key).map(str::to_owned);
    Genome {
        name: word(stanza.get("genome").unwrap_or_default()).to_owned(),
        track_db: text("trackDb"),
        two_bit: text("twoBitPath"),
        organism: text("organism").unwrap_or_default(),
        description: text("description").unwrap_or_default(),
        position: text("defaultPos"),
    }
}

pub fn genomes(text: &str) -> Vec<Genome> {
    stanzas(text)
        .0
        .iter()
        .filter(|stanza| stanza.first() == Some("genome"))
        .map(genome)
        .collect()
}

fn track(stanza: &Stanza) -> Track {
    let name = word(stanza.get("track").unwrap_or_default());
    let short = stanza.get("shortLabel").unwrap_or(name);
    let on = |key: &str| stanza.get(key).is_some_and(|value| word(value) == "on");
    let member = stanza
        .get("superTrack")
        .map(word)
        .filter(|value| *value != "on" && !value.is_empty());
    Track {
        name: name.to_owned(),
        short: short.to_owned(),
        long: stanza.get("longLabel").unwrap_or(short).to_owned(),
        kind: word(stanza.get("type").unwrap_or_default()).to_owned(),
        data: stanza.get("bigDataUrl").map(str::to_owned),
        index: stanza.get("bigDataIndex").map(str::to_owned),
        parent: stanza.get("parent").map(word).or(member).map(str::to_owned),
        container: on("compositeTrack") || on("superTrack") || stanza.get("view").is_some(),
    }
}

/// The tracks of a trackDb.txt file, and the files that it includes.
pub fn tracks(text: &str) -> (Vec<Track>, Vec<String>) {
    let (found, included) = stanzas(text);
    let tracks = found
        .iter()
        .filter(|stanza| stanza.first() == Some("track"))
        .map(track)
        .collect();
    (tracks, included)
}

#[cfg(test)]
mod tests {
    use super::{Genome, Hub, HubFile, Track, genomes, hub, stanzas, tracks};

    #[test]
    fn stanzas_end_at_blank_lines_and_skip_comments() {
        let text = "# a hub\nhub mine\nshortLabel My hub\n\n  track a\n  type bigBed 6 +\nlongLabel A long \\\n  label\n\ninclude more.txt\n";
        let (found, included) = stanzas(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].get("shortLabel"), Some("My hub"));
        assert_eq!(found[1].get("type"), Some("bigBed 6 +"));
        assert_eq!(found[1].get("longLabel"), Some("A long label"));
        assert_eq!(found[1].get("missing"), None);
        assert_eq!(included, ["more.txt"]);
    }

    #[test]
    fn hubs_name_their_genomes_file() {
        let text = "hub mine\nshortLabel My hub\nlongLabel My long hub\ngenomesFile genomes.txt\nemail a@b.c\n";
        let HubFile {
            hub: found,
            genomes,
            tracks,
        } = hub(text).unwrap();
        assert_eq!(
            found,
            Hub {
                name: "mine".to_owned(),
                short: "My hub".to_owned(),
                long: "My long hub".to_owned(),
                genomes: Some("genomes.txt".to_owned()),
                one_file: false,
            }
        );
        assert!(genomes.is_empty() && tracks.is_empty());
        assert!(hub("track a\ntype bigWig\n").is_none());
    }

    #[test]
    fn genomes_point_at_their_tracks_and_sequence() {
        let text = "genome hg38\ntrackDb hg38/trackDb.txt\n\ngenome myAsm\ntrackDb myAsm/trackDb.txt\ntwoBitPath myAsm/myAsm.2bit\norganism Fly\ndescription My assembly\ndefaultPos chr1:1-1000\n";
        let found = genomes(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "hg38");
        assert_eq!(found[0].two_bit, None);
        assert_eq!(
            found[1],
            Genome {
                name: "myAsm".to_owned(),
                track_db: Some("myAsm/trackDb.txt".to_owned()),
                two_bit: Some("myAsm/myAsm.2bit".to_owned()),
                organism: "Fly".to_owned(),
                description: "My assembly".to_owned(),
                position: Some("chr1:1-1000".to_owned()),
            }
        );
    }

    #[test]
    fn tracks_keep_their_type_data_and_place() {
        let text = "track signals\nsuperTrack on show\nshortLabel Signals\n\n\
            track peaks\ncompositeTrack on\nparent signals\nshortLabel Peaks\ntype bigBed 6\n\n\
            track peaksView\nview PK\nparent peaks\n\n\
            track peak1\nparent peaksView on\ntype bigBed 6 +\nbigDataUrl data/peak1.bb\nshortLabel Peak 1\nlongLabel First peaks\n\n\
            track wave\nsuperTrack signals show\ntype bigWig 0 10\nbigDataUrl wave.bw\nshortLabel Wave\n\n\
            track reads\ntype bam\nbigDataUrl reads.bam\nbigDataIndex reads.bam.bai\nshortLabel Reads\n\n\
            include extra.txt\n";
        let (found, included) = tracks(text);
        assert_eq!(included, ["extra.txt"]);
        let names: Vec<&str> = found.iter().map(|track| track.name.as_str()).collect();
        assert_eq!(
            names,
            ["signals", "peaks", "peaksView", "peak1", "wave", "reads"]
        );
        assert!(found[0].container && found[1].container && found[2].container);
        assert_eq!(
            found[3],
            Track {
                name: "peak1".to_owned(),
                short: "Peak 1".to_owned(),
                long: "First peaks".to_owned(),
                kind: "bigBed".to_owned(),
                data: Some("data/peak1.bb".to_owned()),
                index: None,
                parent: Some("peaksView".to_owned()),
                container: false,
            }
        );
        assert_eq!(found[1].parent.as_deref(), Some("signals"));
        assert_eq!(found[4].parent.as_deref(), Some("signals"));
        assert_eq!(found[4].kind, "bigWig");
        assert_eq!(found[4].long, "Wave");
        assert_eq!(found[5].index.as_deref(), Some("reads.bam.bai"));
    }

    #[test]
    fn one_file_hubs_hold_genomes_and_tracks() {
        let text = "hub single\nshortLabel One\nlongLabel One file\nuseOneFile on\nemail a@b.c\n\n\
            genome hg38\n\n\
            track a\ntype bigBed\nbigDataUrl a.bb\nshortLabel A\n\n\
            genome mm39\n\n\
            track b\ntype bigWig\nbigDataUrl b.bw\nshortLabel B\n";
        let HubFile {
            hub: found,
            genomes,
            tracks,
        } = hub(text).unwrap();
        assert!(found.one_file);
        let names: Vec<&str> = genomes.iter().map(|genome| genome.name.as_str()).collect();
        assert_eq!(names, ["hg38", "mm39"]);
        let placed: Vec<(&str, &str)> = tracks
            .iter()
            .map(|(genome, track)| (genome.as_str(), track.name.as_str()))
            .collect();
        assert_eq!(placed, [("hg38", "a"), ("mm39", "b")]);
    }
}
