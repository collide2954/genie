//! Protein domain tracks: the domains of an InterProScan or `pfam_scan.pl` file on the coding
//! exons of the transcripts that the other feature tracks of a panel show. A domain lies on a
//! transcript when the transcript names the protein of the domain, by a protein ID or by a
//! transcript ID without its version, and when its coding part has the length of the protein.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash as _, Hasher as _};
use std::io::{BufReader, Read as _};
use std::sync::Arc;

use genie_core::{Feature, Region, Viewport, effect};
use genie_formats::domains::{self, Domain, Layout};
use genie_formats::fields::Field;
use genie_formats::gzip;

use super::made::COLORS;
use super::{
    Content, FeatureFile, Format, Loaded, Track, feature_fields, head, inner_extension,
    is_compressed,
};
use crate::location::Location;

/// The largest file of domains that genie reads.
const LIMIT: u64 = 512 << 20;
/// The file types of domain files, also compressed: InterProScan writes `.tsv`, and the output
/// of `pfam_scan.pl` has the name that somebody gives it.
const NAMES: [&str; 5] = ["tsv", "txt", "out", "pfam", "pfamscan"];
/// The field of a domain with the IDs of the transcripts that it lies on.
pub const TRANSCRIPTS: &str = "Transcripts";
/// The labels of the fields of a domain, in the order of its extra text.
const FIELDS: [&str; 7] = [
    "Protein",
    "Amino acids",
    "Database",
    "Accession",
    "Description",
    "InterPro",
    TRANSCRIPTS,
];
/// The databases whose domains name a variant change first, best first.
const BEST: [&str; 4] = ["Pfam", "SMART", "CDD", "ProSiteProfiles"];
/// What a domain track says when no other feature track is open.
pub const NO_GENES: &str =
    "Open a gene track, as GENCODE or RefSeq, to put the domains on the exons of its transcripts.";
/// What a domain track says when no transcript in view has a protein of its file.
pub const NO_MATCH: &str = "No transcript in view has a protein of this file. genie compares protein IDs and transcript IDs without their versions.";

/// True for the names that only the output of `pfam_scan.pl` has: `.pfam`, `.pfamscan` and
/// `.out`, also compressed. genie reads the first lines of `.tsv` and `.txt` files to find
/// domain files.
pub fn is_name(name: &str) -> bool {
    inner_extension(name, &["pfam", "pfamscan", "out"])
}

/// The layout of a file of domains, by its type and its first lines. For a file on the web, this
/// reads its first lines.
pub fn layout(location: &Location, name: &str) -> Option<Layout> {
    if !inner_extension(name, &NAMES) {
        return None;
    }
    Layout::sniff(&head(location, 50))
}

/// The domains of a file, the domains of each key of a protein, and the databases in the order
/// of the file, which give the colors.
#[derive(Debug)]
pub struct DomainFile {
    domains: Vec<Domain>,
    proteins: HashMap<String, Vec<usize>>,
    databases: Vec<String>,
}

impl DomainFile {
    pub fn open(location: &Location, name: &str, layout: Layout) -> Result<Self, String> {
        if location.size().is_ok_and(|size| size > LIMIT) {
            return Err(format!(
                "The domain file {name} is larger than {} MB.",
                LIMIT >> 20
            ));
        }
        let file = location.open()?;
        let mut text = String::new();
        let read = if is_compressed(name) {
            gzip::Reader::new(BufReader::new(file)).read_to_string(&mut text)
        } else {
            BufReader::new(file).read_to_string(&mut text)
        };
        read.map_err(|error| format!("genie cannot read the domain file {name}. {error}"))?;
        let file = Self::new(domains::parse(&text, layout));
        if file.domains.is_empty() {
            return Err(format!("The file {name} has no protein domains."));
        }
        Ok(file)
    }

    pub fn new(domains: Vec<Domain>) -> Self {
        let mut proteins: HashMap<String, Vec<usize>> = HashMap::new();
        let mut databases: Vec<String> = Vec::new();
        for (at, domain) in domains.iter().enumerate() {
            for key in protein_keys(&domain.protein) {
                proteins.entry(key).or_default().push(at);
            }
            if !databases.contains(&domain.database) {
                databases.push(domain.database.clone());
            }
        }
        Self {
            domains,
            proteins,
            databases,
        }
    }

    /// True when the domains come from more than one database, which have a color each.
    pub fn has_colors(&self) -> bool {
        self.databases.len() > 1
    }

    fn color(&self, database: &str) -> Option<[u8; 3]> {
        if !self.has_colors() {
            return None;
        }
        let at = self.databases.iter().position(|known| known == database)?;
        Some(COLORS[at % COLORS.len()])
    }

    /// The domains of the protein of a transcript: those of the first of its keys that names a
    /// protein of the file.
    fn of(&self, transcript: &Feature, fields: &[Field]) -> Option<&[usize]> {
        keys(transcript, fields)
            .iter()
            .find_map(|key| self.proteins.get(key).map(Vec::as_slice))
    }

    /// The domains on the coding exons of transcripts with the fields of their files, and the
    /// number of transcripts that have a protein of the file. A domain on the same exons of two
    /// transcripts shows once, with the two transcripts in its fields.
    pub fn features(&self, transcripts: &[(Feature, Vec<Field>)]) -> (Vec<Feature>, usize) {
        let mut placed: Vec<(usize, Feature, Vec<String>)> = Vec::new();
        let mut seen: HashMap<(usize, Vec<Region>), usize> = HashMap::new();
        let mut matched = 0;
        for (transcript, fields) in transcripts {
            let Some(indices) = self.of(transcript, fields) else {
                continue;
            };
            let Some(codons) = effect::coding_length(transcript).map(|length| length / 3) else {
                continue;
            };
            let fits = |domain: &Domain| {
                domain
                    .length
                    .is_none_or(|length| codons == length || codons == length + 1)
            };
            let id = transcript_id(transcript, fields);
            let mut any = false;
            for &at in indices {
                let domain = &self.domains[at];
                if !fits(domain) {
                    continue;
                }
                let Some(blocks) = effect::protein_blocks(transcript, (domain.first, domain.last))
                else {
                    continue;
                };
                let Some(span) = blocks.iter().copied().reduce(Region::hull) else {
                    continue;
                };
                any = true;
                if let Some(&known) = seen.get(&(at, blocks.clone())) {
                    if !placed[known].2.contains(&id) {
                        placed[known].2.push(id.clone());
                    }
                    continue;
                }
                seen.insert((at, blocks.clone()), placed.len());
                let feature = Feature::new(domain.title(), span, transcript.strand())
                    .with_blocks(blocks)
                    .with_color(self.color(&domain.database));
                placed.push((at, feature, vec![id.clone()]));
            }
            matched += usize::from(any);
        }
        let mut features: Vec<Feature> = placed
            .into_iter()
            .map(|(at, feature, ids)| {
                let extra = extra(&self.domains[at], &ids);
                feature.with_extra(extra)
            })
            .collect();
        features.sort_by_key(|feature| (feature.span().start(), feature.span().end()));
        (features, matched)
    }

    /// The fields of a domain for its card and the filters.
    pub fn fields(extra: &str) -> Vec<Field> {
        FIELDS
            .iter()
            .zip(extra.split('\t'))
            .filter(|(_, value)| !value.is_empty())
            .map(|(label, value)| Field {
                label: (*label).to_owned(),
                value: value.to_owned(),
            })
            .collect()
    }
}

/// The extra text of a domain on transcripts: its fields, split by tabs.
fn extra(domain: &Domain, transcripts: &[String]) -> String {
    let clean = |text: &str| text.replace('\t', " ");
    let description = if domain.description == domain.name {
        String::new()
    } else {
        clean(&domain.description)
    };
    let interpro = domain
        .interpro
        .as_ref()
        .map_or_else(String::new, |(entry, about)| {
            if about.is_empty() {
                entry.clone()
            } else {
                format!("{entry} {}", clean(about))
            }
        });
    [
        clean(&domain.protein),
        format!("{}-{}", domain.first, domain.last),
        clean(&domain.database),
        clean(&domain.accession),
        description,
        interpro,
        transcripts.join(", "),
    ]
    .join("\t")
}

/// The keys of a protein or a transcript ID, in lower case and without the prefix of a GFF3 ID
/// or of a database link: the ID with its version, then the ID without it, as
/// `ensp00000269305.4` and `ensp00000269305` for `ENSP00000269305.4`.
fn forms(id: &str) -> Vec<String> {
    let id = id.trim();
    let id = id.rsplit_once(':').map_or(id, |(_, after)| after);
    let lower = id.to_ascii_lowercase();
    let lower = ["rna-", "cds-"]
        .iter()
        .find_map(|prefix| lower.strip_prefix(prefix).map(str::to_owned))
        .unwrap_or(lower);
    if lower.is_empty() {
        return Vec::new();
    }
    match lower.rsplit_once('.') {
        Some((stem, version))
            if !stem.is_empty()
                && !version.is_empty()
                && version.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            let stem = stem.to_owned();
            vec![lower, stem]
        }
        _ => vec![lower],
    }
}

/// The keys of the protein of a domain: the forms of its ID, of the accession and the entry
/// name of a UniProt ID such as `sp|P04637|P53_HUMAN`, or of the protein and the transcript of
/// a GENCODE ID such as `ENSP00000269305.4|ENST00000269305.9|…`.
fn protein_keys(protein: &str) -> Vec<String> {
    let parts: Vec<&str> = protein.split('|').collect();
    let ids: Vec<&str> = match parts.as_slice() {
        [database, accession, name, ..] if matches!(*database, "sp" | "tr") => {
            vec![accession, name]
        }
        [first, second, ..] => vec![first, second],
        _ => vec![protein],
    };
    let mut found: Vec<String> = Vec::new();
    for form in ids.into_iter().flat_map(forms) {
        if !found.contains(&form) {
            found.push(form);
        }
    }
    found
}

/// The keys that can name the protein of a transcript, best first: the values of its fields of
/// protein IDs and of database links, then its transcript IDs and its name, each with its
/// version first.
fn keys(transcript: &Feature, fields: &[Field]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut add = |text: &str| {
        for part in text.split([',', ';', ' ']).filter(|part| !part.is_empty()) {
            for form in forms(part) {
                if !found.contains(&form) {
                    found.push(form);
                }
            }
        }
    };
    let proteins = fields.iter().filter(|field| {
        let label = field.label.to_ascii_lowercase();
        label.contains("prot") || label == "dbxref"
    });
    for field in proteins {
        add(&field.value);
    }
    let ids = fields
        .iter()
        .filter(|field| matches!(field.label.as_str(), "transcript_id" | "ID" | "Name"));
    for field in ids {
        add(&field.value);
    }
    if let Some(id) = transcript.id() {
        add(id);
    }
    add(transcript.name());
    found
}

/// The ID of a transcript in the fields of its domains: its transcript ID, else its name.
pub fn transcript_id(transcript: &Feature, fields: &[Field]) -> String {
    let field = |label: &str| {
        fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| field.value.as_str())
    };
    let id = field("transcript_id")
        .or_else(|| transcript.id())
        .or_else(|| field("ID").map(|id| id.rsplit_once(':').map_or(id, |(_, after)| after)))
        .unwrap_or_else(|| transcript.name());
    id.to_owned()
}

/// The words of a domain in a change line of a variant card, as "in P53 (PF00870, aa 95-289)",
/// and its rank: the best databases first, then the shorter domains.
pub fn words(feature: &Feature, fields: &[Field]) -> (String, (usize, u64)) {
    let field = |label: &str| {
        fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| field.value.as_str())
    };
    let accession = field("Accession").unwrap_or_default();
    let name = feature
        .name()
        .strip_suffix(&format!(" ({accession})"))
        .unwrap_or(feature.name());
    let parts: Vec<String> = [
        (!accession.is_empty() && accession != name).then(|| accession.to_owned()),
        field("Amino acids").map(|range| format!("aa {range}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let words = if parts.is_empty() {
        format!("in {name}")
    } else {
        format!("in {name} ({})", parts.join(", "))
    };
    let database = field("Database").unwrap_or_default();
    let best = BEST
        .iter()
        .position(|known| known.eq_ignore_ascii_case(database))
        .unwrap_or(BEST.len());
    let length = feature.blocks().iter().copied().map(Region::len).sum();
    (words, (best, length))
}

/// Gives the domain tracks of a panel the domains of the transcripts that the other feature
/// tracks of the panel show on a contig, when those transcripts or the options of the track
/// changed.
pub fn refresh(tracks: &mut [Track], panel: usize, (contig, viewport): (usize, Viewport)) {
    let domains: Vec<usize> = tracks
        .iter()
        .enumerate()
        .filter(|(_, track)| track.source.has_domains() && !track.hidden)
        .map(|(at, _)| at)
        .collect();
    if domains.is_empty() {
        return;
    }
    let genes =
        |track: &Track| !track.hidden && track.source.has_features() && !track.source.has_domains();
    let open = tracks.iter().any(genes);
    let inputs: Vec<(&Track, &Loaded)> = tracks
        .iter()
        .filter(|track| genes(track))
        .filter_map(|track| Some((track, track.slot(panel).data(contig)?)))
        .filter(|(_, data)| matches!(data.content, Content::Features(_)))
        .collect();
    let mut hasher = DefaultHasher::new();
    (contig, open).hash(&mut hasher);
    for (track, data) in &inputs {
        (track.id, std::ptr::from_ref(*data) as usize, data.region).hash(&mut hasher);
    }
    let basis = hasher.finish();
    let stale: Vec<usize> = domains
        .into_iter()
        .filter(|&at| {
            let track = &tracks[at];
            let slot = track.slot(panel);
            slot.basis != Some(basis)
                || slot
                    .data(contig)
                    .is_none_or(|data| data.options != track.options)
        })
        .collect();
    if stale.is_empty() {
        return;
    }
    let visible = viewport.visible();
    let region = inputs
        .iter()
        .map(|(_, data)| data.region)
        .reduce(Region::hull)
        .unwrap_or_else(|| visible.expand(visible.len(), viewport.length()));
    let mut transcripts: Vec<(Feature, Vec<Field>)> = Vec::new();
    let mut known: HashSet<(Region, String)> = HashSet::new();
    for (track, data) in &inputs {
        let Content::Features(annotation) = &data.content else {
            continue;
        };
        for feature in annotation
            .features
            .iter()
            .filter(|feature| effect::readable(feature))
        {
            let fields = feature_fields(&track.source, feature);
            if known.insert((feature.span(), transcript_id(feature, &fields))) {
                transcripts.push((feature.clone(), fields));
            }
        }
    }
    for at in stale {
        let track = &mut tracks[at];
        let source = Arc::clone(&track.source);
        let Format::Features(FeatureFile::Domains(file)) = &source.format else {
            continue;
        };
        let (features, matched) = file.features(&transcripts);
        let hint = if !open {
            Some(NO_GENES)
        } else if matched == 0 && !transcripts.is_empty() {
            Some(NO_MATCH)
        } else {
            None
        };
        let mut loaded = Loaded::features(region, features);
        loaded.options = track.options;
        let shown = track.derive(contig, &loaded);
        let slot = track.slot_mut(panel);
        slot.loaded = Some((contig, Arc::new(loaded)));
        (slot.shown, slot.wanted, slot.order) = (shown, None, None);
        slot.error = hint.map(str::to_owned);
        slot.basis = Some(basis);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use genie_core::{Assembly, Contig, Feature, Region, Strand};
    use genie_formats::domains::{Domain, Layout};
    use genie_formats::fields::Field;

    use super::{DomainFile, forms, keys, layout, protein_keys, transcript_id, words};
    use crate::location::Location;
    use crate::scratch::Scratch;
    use crate::track::made::COLORS;
    use crate::track::{Track, open_with};

    const INTERPRO: &str = "ENSP1.3\tmd5\t16\tPfam\tPF00001\tKinase domain\t1\t6\t1.2E-9\tT\t01-01-2026\tIPR000001\tKinase\n";
    const PFAM: &str = "# pfam_scan.pl\nENST4    1    6    1    6 PF00001.9  Kinase  Domain  1  6  6  10.0  1e-5  1  No_clan\n";

    fn field(label: &str, value: &str) -> Field {
        Field {
            label: label.to_owned(),
            value: value.to_owned(),
        }
    }

    #[test]
    fn domain_files_open_by_their_type_and_their_lines() {
        let scratch = Scratch::new("domain-files");
        let file = |name: &str, text: &str| Location::File(scratch.file(name, text));
        let interpro = file("proteins.tsv", INTERPRO);
        assert_eq!(
            layout(&interpro, "proteins.tsv"),
            Some(Layout::InterProScan)
        );
        let pfam = file("proteins.pfam", PFAM);
        assert_eq!(layout(&pfam, "proteins.pfam"), Some(Layout::PfamScan));
        let table = file("groups.tsv", "sample\tpop\nS1\tEUR\n");
        assert_eq!(layout(&table, "groups.tsv"), None);
        let bed = file("genes.bed", INTERPRO);
        assert_eq!(layout(&bed, "genes.bed"), None);
        let source = Arc::new(open_with(interpro, None).unwrap());
        assert!(source.has_domains() && source.has_features() && !source.can_jump());
        let assembly = Assembly::new(
            ["chr1", "chr2"]
                .into_iter()
                .map(|name| Contig {
                    name: name.to_owned(),
                    length: 1_000,
                })
                .collect(),
        );
        let track = Track::new(0, source, &assembly);
        assert_eq!(
            track.contigs,
            [Some("chr1".to_owned()), Some("chr2".to_owned())]
        );
        let empty = file("empty.tsv", "# nothing\n");
        assert!(open_with(empty, None).is_err());
    }

    #[test]
    fn keys_have_the_version_and_then_none() {
        assert_eq!(
            forms("ENSP00000269305.4"),
            ["ensp00000269305.4", "ensp00000269305"]
        );
        assert_eq!(forms("transcript:ENST00000269305"), ["enst00000269305"]);
        assert_eq!(forms("rna-NM_000546.6"), ["nm_000546.6", "nm_000546"]);
        assert_eq!(forms("cds-NP_000537.3"), ["np_000537.3", "np_000537"]);
        assert_eq!(forms("UniProtKB/Swiss-Prot:P04637"), ["p04637"]);
        assert_eq!(forms("P04637-2"), ["p04637-2"]);
        assert_eq!(forms("gene.v2"), ["gene.v2"]);
        assert_eq!(forms(" "), Vec::<String>::new());
        assert_eq!(protein_keys("sp|P04637|P53_HUMAN"), ["p04637", "p53_human"]);
        assert_eq!(
            protein_keys("ENSP1.2|ENST1.3|ENSG1.1|OTTHUMG1|OTTHUMT1|TP53-201|TP53|393"),
            ["ensp1.2", "ensp1", "enst1.3", "enst1"]
        );
        let transcript = Feature::new("TP53", Region::between(0, 30), Strand::Forward);
        let fields = [
            field("transcript_id", "ENST1.2"),
            field("protein_id", "ENSP1.2"),
            field("Dbxref", "GeneID:7157,Genbank:NP_1.1"),
        ];
        assert_eq!(
            keys(&transcript, &fields),
            [
                "ensp1.2", "ensp1", "7157", "np_1.1", "np_1", "enst1.2", "enst1", "tp53"
            ]
        );
        assert_eq!(transcript_id(&transcript, &fields), "ENST1.2");
        let labeled =
            Feature::new("ENST9.1", Region::between(0, 30), Strand::Forward).with_label("GENE9");
        assert_eq!(transcript_id(&labeled, &[]), "ENST9.1");
        assert_eq!(keys(&labeled, &[]), ["enst9.1", "enst9", "gene9"]);
    }

    /// The domains of the exact version of a protein go first: the isoforms .1 and .2 of a
    /// protein of TAIR differ.
    #[test]
    fn exact_versions_go_first() {
        let kinase = ("Pfam", "PF00001", "Kinase domain");
        let file = DomainFile::new(vec![
            domain("AT1G01010.1", (1, 6), kinase),
            domain("AT1G01010.2", (2, 7), kinase),
        ]);
        let first = transcript(Strand::Forward, &[("protein_id", "AT1G01010.1")]);
        let (features, _) = file.features(std::slice::from_ref(&first));
        let blocks: Vec<Vec<Region>> = features
            .iter()
            .map(|feature| feature.blocks().to_vec())
            .collect();
        assert_eq!(blocks, [vec![Region::between(112, 130)]]);
    }

    fn domain(protein: &str, (first, last): (u64, u64), signature: (&str, &str, &str)) -> Domain {
        let (database, accession, name) = signature;
        Domain {
            protein: protein.to_owned(),
            first,
            last,
            length: Some(16),
            database: database.to_owned(),
            accession: accession.to_owned(),
            name: name.to_owned(),
            description: name.to_owned(),
            interpro: Some(("IPR000001".to_owned(), "Kinase".to_owned())),
        }
    }

    /// A transcript with exons at 100-130 and 200-260 and a coding part from 112 to 230: 16
    /// codons, 6 in the first exon.
    fn transcript(strand: Strand, fields: &[(&str, &str)]) -> (Feature, Vec<Field>) {
        let feature = Feature::new("TP53", Region::between(100, 260), strand)
            .with_blocks(vec![Region::between(100, 130), Region::between(200, 260)])
            .with_coding(Some(Region::between(112, 230)));
        let fields = fields
            .iter()
            .map(|(label, value)| field(label, value))
            .collect();
        (feature, fields)
    }

    /// Domains lie on the coding exons of the transcripts whose protein or transcript ID they
    /// name, when the protein has the length of the coding part. A domain on the same exons of
    /// two transcripts shows once.
    #[test]
    fn domains_lie_on_the_coding_exons_of_their_transcripts() {
        let kinase = ("Pfam", "PF00001", "Kinase domain");
        let mut other = domain("ENSP9", (1, 6), kinase);
        other.length = Some(30);
        let mut loose = domain("ENST4", (1, 6), kinase);
        (loose.length, loose.interpro) = (None, None);
        let file = DomainFile::new(vec![
            domain("ENSP1.3", (1, 6), kinase),
            domain("ENSP1.3", (6, 7), ("Gene3D", "G3DSA:1.10", "G3DSA:1.10")),
            domain("ENSP1.3", (10, 20), kinase),
            other,
            loose,
        ]);
        assert!(file.has_colors());
        let transcripts = [
            transcript(
                Strand::Forward,
                &[("transcript_id", "ENST1.1"), ("protein_id", "ENSP1.3")],
            ),
            transcript(
                Strand::Forward,
                &[("transcript_id", "ENST3.1"), ("protein_id", "ENSP1.2")],
            ),
            transcript(Strand::Forward, &[("protein_id", "ENSP9.1")]),
            transcript(Strand::Reverse, &[("transcript_id", "ENST4")]),
        ];
        let (features, matched) = file.features(&transcripts);
        assert_eq!(matched, 3);
        let shown: Vec<(&str, Vec<Region>, Strand)> = features
            .iter()
            .map(|feature| (feature.name(), feature.blocks().to_vec(), feature.strand()))
            .collect();
        assert_eq!(
            shown,
            [
                (
                    "Kinase domain (PF00001)",
                    vec![Region::between(112, 130)],
                    Strand::Forward
                ),
                (
                    "G3DSA:1.10",
                    vec![Region::between(127, 130), Region::between(200, 203)],
                    Strand::Forward
                ),
                (
                    "Kinase domain (PF00001)",
                    vec![Region::between(212, 230)],
                    Strand::Reverse
                ),
            ]
        );
        assert_eq!(features[0].color(), Some(COLORS[0]));
        assert_eq!(features[1].color(), Some(COLORS[1]));
        let fields: Vec<(String, String)> = DomainFile::fields(features[0].extra())
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect();
        let pair = |label: &str, value: &str| (label.to_owned(), value.to_owned());
        assert_eq!(
            fields,
            [
                pair("Protein", "ENSP1.3"),
                pair("Amino acids", "1-6"),
                pair("Database", "Pfam"),
                pair("Accession", "PF00001"),
                pair("InterPro", "IPR000001 Kinase"),
                pair("Transcripts", "ENST1.1, ENST3.1"),
            ]
        );
    }

    /// A change line names a domain with its accession and its amino acids. Pfam goes before
    /// the other databases, and a shorter domain before a longer one.
    #[test]
    fn words_name_a_domain_for_change_lines() {
        let kinase = ("Pfam", "PF00001", "Kinase domain");
        let gene3d = ("Gene3D", "G3DSA:1.10", "G3DSA:1.10");
        let file = DomainFile::new(vec![
            domain("ENSP1", (1, 6), kinase),
            domain("ENSP1", (6, 7), gene3d),
        ]);
        let forward = transcript(Strand::Forward, &[("protein_id", "ENSP1")]);
        let (features, _) = file.features(std::slice::from_ref(&forward));
        let of = |feature: &Feature| words(feature, &DomainFile::fields(feature.extra()));
        assert_eq!(
            of(&features[0]),
            ("in Kinase domain (PF00001, aa 1-6)".to_owned(), (0, 18))
        );
        assert_eq!(
            of(&features[1]),
            ("in G3DSA:1.10 (aa 6-7)".to_owned(), (4, 6))
        );
        let ucsc = Feature::new("Protein kinase", Region::between(0, 30), Strand::Forward);
        assert_eq!(words(&ucsc, &[]), ("in Protein kinase".to_owned(), (4, 30)));
    }
}
