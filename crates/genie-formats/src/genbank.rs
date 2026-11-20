//! GenBank and EMBL flat files: the sequence and the features of each record. Genes join with
//! their mRNAs and CDSs into gene models, as a track of transcripts shows them.

use std::io::BufRead;

use genie_core::{Feature, Region, Strand};

use crate::fields::Field;
use crate::{Error, Reason};

/// A record of a file: its name, its shape, its bases in upper case, its length, and its
/// features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub name: String,
    pub circular: bool,
    pub bases: Vec<u8>,
    pub length: u64,
    pub features: Vec<Feature>,
}

/// The kinds of features that are transcripts of a gene.
const RNA: [&str; 8] = [
    "mRNA",
    "ncRNA",
    "rRNA",
    "tRNA",
    "misc_RNA",
    "precursor_RNA",
    "tmRNA",
    "lnc_RNA",
];
/// The longest name that a note gives a feature.
const NOTE_NAME: usize = 40;

/// True for the names of GenBank and EMBL files, also compressed with gzip.
pub fn is_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".gz").unwrap_or(&lower);
    stem.rsplit_once('.').is_some_and(|(_, extension)| {
        matches!(
            extension,
            "gb" | "gbk" | "gbff" | "genbank" | "embl" | "emb"
        )
    })
}

/// True when the first bytes of a file are the start of a GenBank or EMBL record.
pub fn sniff(head: &[u8]) -> bool {
    head.starts_with(b"LOCUS") || head.starts_with(b"ID   ")
}

/// A feature as the file writes it: its key, its location and its qualifiers. A qualifier
/// without a value has an empty value.
#[derive(Debug, Default)]
struct Raw {
    key: String,
    location: String,
    qualifiers: Vec<(String, String)>,
    /// Whether the value of the last qualifier waits for its closing quote.
    open: bool,
}

impl Raw {
    fn get(&self, key: &str) -> Option<&str> {
        self.qualifiers
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, value)| value.as_str())
    }

    /// Adds a line of qualifiers or of the location.
    fn add(&mut self, text: &str) {
        if self.open {
            if let Some((_, value)) = self.qualifiers.last_mut() {
                value.push(' ');
                value.push_str(text.trim());
            }
            self.open = !closed(self.qualifiers.last().map_or("", |(_, value)| value));
        } else if let Some(qualifier) = text.strip_prefix('/') {
            let (key, value) = qualifier.split_once('=').unwrap_or((qualifier, ""));
            let value = value.trim();
            match value.strip_prefix('"') {
                Some(quoted) => {
                    self.open = !closed(quoted);
                    self.qualifiers.push((key.to_owned(), quoted.to_owned()));
                }
                None => self.qualifiers.push((key.to_owned(), value.to_owned())),
            }
        } else if self.qualifiers.is_empty() {
            self.location.push_str(text.trim());
        }
    }

    /// The qualifiers with their quotes taken away.
    fn finish(mut self) -> Self {
        for (_, value) in &mut self.qualifiers {
            if let Some(inner) = value.strip_suffix('"') {
                *value = inner.replace("\"\"", "\"");
            }
        }
        self
    }
}

/// True when a quoted value that started on an earlier line has its closing quote: an odd
/// number of quotes at its end, as two quotes stand for one quote in the value.
fn closed(value: &str) -> bool {
    value.bytes().rev().take_while(|&byte| byte == b'"').count() % 2 == 1
}

/// A record while genie reads it.
#[derive(Debug, Default)]
struct Draft {
    locus: String,
    length: u64,
    circular: bool,
    accession: Option<String>,
    version: Option<String>,
    raws: Vec<Raw>,
    bases: Vec<u8>,
}

impl Draft {
    /// A record from its LOCUS line, as `LOCUS NC_045512 29903 bp ss-RNA linear VRL`.
    fn locus(line: &str) -> Self {
        let words: Vec<&str> = line.split_whitespace().collect();
        Self {
            locus: words.get(1).copied().unwrap_or_default().to_owned(),
            length: words.get(2).and_then(|word| word.parse().ok()).unwrap_or(0),
            circular: words.contains(&"circular"),
            ..Self::default()
        }
    }

    /// A record from its EMBL ID line, as `ID   X56734; SV 1; linear; mRNA; STD; PLN; 1859 BP.`
    fn embl(line: &str) -> Self {
        let parts: Vec<&str> = line[2..].split(';').map(str::trim).collect();
        let accession = parts.first().copied().unwrap_or_default();
        let version = parts
            .iter()
            .find_map(|part| part.strip_prefix("SV "))
            .map(|number| format!("{accession}.{}", number.trim()));
        let length = parts
            .last()
            .and_then(|part| part.split_whitespace().next())
            .and_then(|word| word.parse().ok())
            .unwrap_or(0);
        Self {
            locus: accession.to_owned(),
            length,
            circular: parts.contains(&"circular"),
            version,
            ..Self::default()
        }
    }

    fn name(&self) -> String {
        let named =
            |text: &Option<String>| text.clone().filter(|name| !name.is_empty() && name != ".");
        named(&self.version)
            .or_else(|| named(&self.accession))
            .unwrap_or_else(|| self.locus.clone())
    }

    fn finish(self) -> Record {
        let length = if self.bases.is_empty() {
            self.length
        } else {
            self.bases.len() as u64
        };
        let name = self.name();
        let raws = self.raws.into_iter().map(Raw::finish).collect();
        Record {
            name,
            circular: self.circular,
            features: models(raws, self.circular),
            length,
            bases: self.bases,
        }
    }
}

/// Where a line of a record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Header,
    Features,
    Sequence,
}

/// The records of a GenBank or EMBL file.
pub fn read(mut reader: impl BufRead) -> Result<Vec<Record>, Error> {
    let mut records = Vec::new();
    let mut draft: Option<Draft> = None;
    let mut part = Part::Header;
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        if reader.read_until(b'\n', &mut bytes)? == 0 {
            break;
        }
        let text = String::from_utf8_lossy(&bytes);
        let line = text.trim_end_matches(['\n', '\r']);
        if line.starts_with("LOCUS") || line.starts_with("ID   ") {
            if let Some(done) = draft.take() {
                records.push(done.finish());
            }
            draft = Some(if line.starts_with("LOCUS") {
                Draft::locus(line)
            } else {
                Draft::embl(line)
            });
            part = Part::Header;
            continue;
        }
        let Some(current) = draft.as_mut() else {
            if line.trim().is_empty() {
                continue;
            }
            return Err(Error::Invalid(Reason::NotGenBank));
        };
        if line.starts_with("//") {
            if let Some(done) = draft.take() {
                records.push(done.finish());
            }
            continue;
        }
        part = step(current, part, line);
    }
    if let Some(done) = draft {
        records.push(done.finish());
    }
    if records.is_empty() {
        return Err(Error::Invalid(Reason::NotGenBank));
    }
    Ok(records)
}

/// Reads a line of a record, and gives the part of the record of the next line.
fn step(draft: &mut Draft, part: Part, line: &str) -> Part {
    if line.starts_with("ORIGIN") || line.starts_with("SQ   ") {
        return Part::Sequence;
    }
    if part == Part::Sequence {
        draft.bases.extend(
            line.bytes()
                .filter(u8::is_ascii_alphabetic)
                .map(|byte| byte.to_ascii_uppercase()),
        );
        return part;
    }
    let first = |line: &str, tag: &str| {
        line.strip_prefix(tag)
            .and_then(|rest| rest.split([' ', ';']).find(|word| !word.is_empty()))
            .map(str::to_owned)
    };
    if line.starts_with("ACCESSION") {
        draft.accession = first(line, "ACCESSION");
    } else if line.starts_with("VERSION") {
        draft.version = first(line, "VERSION");
    } else if line.starts_with("AC   ") && draft.accession.is_none() {
        draft.accession = first(line, "AC   ");
    } else if line.starts_with("FEATURES") || line.starts_with("FH") {
        return Part::Features;
    } else if let Some(rest) = line.strip_prefix("FT") {
        feature_line(draft, &format!("  {rest}"));
        return Part::Features;
    } else if part == Part::Features && line.starts_with("     ") {
        feature_line(draft, line);
    } else if !line.starts_with(' ') {
        return Part::Header;
    }
    part
}

/// A line of the feature table: a new feature with its key at column 6, or more of the last
/// feature from column 22.
fn feature_line(draft: &mut Draft, line: &str) {
    let key = line.get(5..21).unwrap_or_default().trim();
    let rest = line.get(21..).unwrap_or_default().trim();
    if !key.is_empty() && line.get(5..6).is_none_or(|column| column != " ") {
        draft.raws.push(Raw {
            key: key.to_owned(),
            location: rest.to_owned(),
            ..Raw::default()
        });
    } else if let Some(raw) = draft.raws.last_mut() {
        raw.add(rest);
    }
}

/// The parts of a feature location in INSDC words, such as `complement(join(10..20,30..40))`,
/// in the order of the text, and the strand. Parts on other records are left out.
pub fn location(text: &str) -> Option<(Vec<Region>, Strand)> {
    let text: String = text
        .chars()
        .filter(|letter| !letter.is_whitespace())
        .collect();
    let (parts, rest) = parse(&text, false)?;
    if !rest.is_empty() {
        return None;
    }
    let (spans, strands): (Vec<Region>, Vec<bool>) = parts.into_iter().flatten().unzip();
    if spans.is_empty() {
        return None;
    }
    let reverse = strands.iter().filter(|&&reverse| reverse).count() * 2 > strands.len();
    let strand = if reverse {
        Strand::Reverse
    } else {
        Strand::Forward
    };
    Some((spans, strand))
}

/// The parts of a location and the text after it. A part on another record is None.
type Parts<'a> = (Vec<Option<(Region, bool)>>, &'a str);

fn parse(text: &str, reverse: bool) -> Option<Parts<'_>> {
    for (word, flips) in [("complement(", true), ("join(", false), ("order(", false)] {
        if let Some(inner) = text.strip_prefix(word) {
            let mut parts = Vec::new();
            let mut rest = inner;
            loop {
                let (found, after) = parse(rest, reverse != flips)?;
                parts.extend(found);
                match after.as_bytes().first() {
                    Some(b',') if !flips => rest = &after[1..],
                    Some(b')') => return Some((parts, &after[1..])),
                    _ => return None,
                }
            }
        }
    }
    let end = text.find([',', ')']).unwrap_or(text.len());
    let (range, rest) = text.split_at(end);
    if range.contains(':') {
        return Some((vec![None], rest));
    }
    let number = |word: &str| -> Option<u64> { word.trim_start_matches(['<', '>']).parse().ok() };
    let (start, stop) = match range.split_once("..").or_else(|| range.split_once('^')) {
        Some((start, stop)) => (number(start)?, number(stop)?),
        None => (number(range)?, number(range)?),
    };
    if start == 0 || stop < start {
        return None;
    }
    Some((
        vec![Some((Region::between(start - 1, stop), reverse))],
        rest,
    ))
}

/// A feature with its parts and its strand.
struct Shaped {
    raw: Raw,
    parts: Vec<Region>,
    strand: Strand,
}

impl Shaped {
    fn hull(&self) -> Region {
        self.parts
            .iter()
            .copied()
            .reduce(Region::hull)
            .unwrap_or(Region::between(0, 0))
    }

    /// The gene of the feature: its locus tag, or its gene name.
    fn family(&self) -> Option<&str> {
        self.raw.get("locus_tag").or_else(|| self.raw.get("gene"))
    }

    /// The gaps between the parts in order of place: the introns of a transcript.
    fn introns(&self) -> Vec<Region> {
        let mut parts = self.parts.clone();
        parts.sort_by_key(|part| part.start());
        parts
            .windows(2)
            .filter_map(|pair| Region::new(pair[0].end(), pair[1].start()))
            .filter(|gap| !gap.is_empty())
            .collect()
    }
}

/// True for features of one gene, or for features that do not name a gene.
fn kin(one: &Shaped, other: &Shaped) -> bool {
    match (one.family(), other.family()) {
        (Some(first), Some(second)) => first == second,
        _ => true,
    }
}

fn overlaps(one: Region, other: Region) -> bool {
    one.start() < other.end() && other.start() < one.end()
}

/// True when a CDS fits a transcript: its parts are in the exons, and the introns of the
/// transcript inside the CDS are the introns of the CDS.
fn fits(cds: &Shaped, rna: &Shaped) -> bool {
    let inside = cds.parts.iter().all(|part| {
        rna.parts
            .iter()
            .any(|exon| exon.start() <= part.start() && part.end() <= exon.end())
    });
    let hull = cds.hull();
    let introns: Vec<Region> = rna
        .introns()
        .into_iter()
        .filter(|intron| hull.start() < intron.start() && intron.end() < hull.end())
        .collect();
    inside && introns == cds.introns()
}

/// The features of a record as genie shows them: each transcript with the CDS that fits it,
/// each CDS without a transcript, each gene without transcripts or CDSs, and the other
/// features. The source feature does not show.
fn models(raws: Vec<Raw>, circular: bool) -> Vec<Feature> {
    let shaped: Vec<Shaped> = raws
        .into_iter()
        .filter(|raw| raw.key != "source")
        .filter_map(|raw| {
            let (parts, strand) = location(&raw.location)?;
            Some(Shaped { raw, parts, strand })
        })
        .collect();
    let of = |key: &str| -> Vec<usize> {
        (0..shaped.len())
            .filter(|&at| {
                shaped[at].raw.key == key
                    || (key == "RNA" && RNA.contains(&shaped[at].raw.key.as_str()))
            })
            .collect()
    };
    let (genes, rnas, cdss) = (of("gene"), of("RNA"), of("CDS"));
    let parent = |child: usize| -> Option<usize> {
        let family = shaped[child].family()?;
        genes.iter().copied().find(|&gene| {
            shaped[gene].family() == Some(family)
                && overlaps(shaped[gene].hull(), shaped[child].hull())
        })
    };
    let mut coding: Vec<Option<(Region, usize)>> = vec![None; shaped.len()];
    let mut used = vec![false; shaped.len()];
    for &cds in &cdss {
        let target = rnas.iter().copied().find(|&rna| {
            coding[rna].is_none()
                && kin(&shaped[cds], &shaped[rna])
                && fits(&shaped[cds], &shaped[rna])
        });
        if let Some(rna) = target {
            coding[rna] = Some((shaped[cds].hull(), cds));
            used[cds] = true;
        }
    }
    let mut parents = vec![false; shaped.len()];
    for &child in rnas.iter().chain(&cdss) {
        if let Some(gene) = parent(child) {
            parents[gene] = true;
        }
    }
    let mut features = Vec::new();
    for (at, item) in shaped.iter().enumerate() {
        let key = item.raw.key.as_str();
        if (key == "gene" && parents[at]) || (key == "CDS" && used[at]) {
            continue;
        }
        let mut kept: Vec<&Raw> = vec![&item.raw];
        if let Some((_, cds)) = coding[at] {
            kept.push(&shaped[cds].raw);
        }
        if let Some(gene) = parent(at) {
            kept.push(&shaped[gene].raw);
        }
        let model = key == "gene" || key == "CDS" || RNA.contains(&key);
        let coded = match key {
            "CDS" => Some(Some(item.hull())),
            "gene" => Some(None),
            _ if RNA.contains(&key) => Some(coding[at].map(|(span, _)| span)),
            _ => None,
        };
        let id = match key {
            "CDS" => item.raw.get("protein_id"),
            "gene" => item
                .raw
                .get("locus_tag")
                .filter(|_| item.raw.get("gene").is_some()),
            _ if RNA.contains(&key) => item.raw.get("transcript_id"),
            _ => None,
        };
        let name = name(&kept, model);
        let extra = extra(key, &kept);
        let color = color(&item.raw, item.strand);
        for parts in pieces(&item.parts, circular) {
            let span = parts
                .iter()
                .copied()
                .reduce(Region::hull)
                .unwrap_or(item.hull());
            let mut feature = match id {
                Some(id) => Feature::new(id, span, item.strand).with_label(name.clone()),
                None => Feature::new(name.clone(), span, item.strand),
            };
            if parts.len() > 1 {
                feature = feature.with_blocks(parts);
            }
            if let Some(coding) = coded {
                feature = feature.with_coding(coding);
            }
            features.push(feature.with_color(color).with_extra(extra.clone()));
        }
    }
    features.sort_by_key(|feature| feature.span().start());
    features
}

/// The parts of a feature, in two pieces when it crosses the origin of a circular record: a
/// part that starts before the part ahead of it in the text starts a new piece.
fn pieces(parts: &[Region], circular: bool) -> Vec<Vec<Region>> {
    let mut pieces: Vec<Vec<Region>> = vec![Vec::new()];
    for (at, part) in parts.iter().enumerate() {
        let wraps = circular && at > 0 && part.start() < parts[at - 1].start();
        if wraps {
            pieces.push(Vec::new());
        }
        if let Some(piece) = pieces.last_mut() {
            piece.push(*part);
        }
    }
    pieces
}

/// The name of a feature: the gene of a gene model, else a label, a product or a note.
fn name(kept: &[&Raw], model: bool) -> String {
    let find = |key: &str| {
        kept.iter()
            .find_map(|raw| raw.get(key))
            .filter(|text| !text.is_empty())
    };
    let order: &[&str] = if model {
        &["gene", "locus_tag", "label", "product", "standard_name"]
    } else {
        &["label", "gene", "locus_tag", "product", "standard_name"]
    };
    order
        .iter()
        .find_map(|key| find(key))
        .map(str::to_owned)
        .or_else(|| {
            find("note").filter(|note| !is_style(note)).map(|note| {
                let mut short: String = note.chars().take(NOTE_NAME).collect();
                if note.chars().count() > NOTE_NAME {
                    short.push('…');
                }
                short
            })
        })
        .unwrap_or_else(|| kept[0].key.clone())
}

/// True for a note that only sets the look of a feature in SnapGene, as
/// `color: #ffef86; direction: LEFT`.
fn is_style(note: &str) -> bool {
    note.trim_start().starts_with("color:")
}

/// True for qualifiers that a card does not show: the protein of a CDS, and colors.
fn hidden(key: &str, value: &str) -> bool {
    matches!(key, "translation" | "color" | "colour")
        || key.starts_with("ApEinfo")
        || (key == "note" && is_style(value))
}

/// The fields of a feature, as `key=value` pairs between tabs: its type, its qualifiers, and
/// the qualifiers of its CDS and its gene that it does not have.
fn extra(key: &str, kept: &[&Raw]) -> String {
    let mut pairs: Vec<(String, String)> = vec![("type".to_owned(), key.to_owned())];
    for (place, raw) in kept.iter().enumerate() {
        let mut own: Vec<(String, String)> = Vec::new();
        for (key, value) in &raw.qualifiers {
            if hidden(key, value) || (place > 0 && pairs.iter().any(|(known, _)| known == key)) {
                continue;
            }
            let value = value.replace(['\t', '\n', '\r'], " ");
            match own.iter_mut().find(|(known, _)| known == key) {
                Some((_, values)) => {
                    values.push_str(", ");
                    values.push_str(&value);
                }
                None => own.push((key.clone(), value)),
            }
        }
        pairs.extend(own);
    }
    pairs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\t")
}

/// The color of a feature from the qualifiers of SnapGene, ApE, Benchling and Artemis: a
/// color for its strand, a color, or a note of SnapGene with a color.
fn color(raw: &Raw, strand: Strand) -> Option<[u8; 3]> {
    let side = if strand == Strand::Reverse {
        "ApEinfo_revcolor"
    } else {
        "ApEinfo_fwdcolor"
    };
    [side, "ApEinfo_fwdcolor", "color", "colour"]
        .iter()
        .find_map(|key| raw.get(key).and_then(rgb))
        .or_else(|| {
            raw.qualifiers
                .iter()
                .filter(|(key, value)| key == "note" && is_style(value))
                .find_map(|(_, value)| {
                    let color = value.trim_start().strip_prefix("color:")?;
                    rgb(color.split(';').next()?)
                })
        })
}

/// A color as `#rrggbb`, or as three numbers from 0 to 255.
fn rgb(text: &str) -> Option<[u8; 3]> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('#') {
        if hex.len() != 6 {
            return None;
        }
        let channel = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
        return Some([channel(0)?, channel(2)?, channel(4)?]);
    }
    let numbers: Vec<u8> = text
        .split([' ', ','])
        .filter(|word| !word.is_empty())
        .map(|word| word.parse().ok())
        .collect::<Option<_>>()?;
    <[u8; 3]>::try_from(numbers).ok()
}

/// The fields of a feature for its card: its type and its qualifiers.
pub fn fields(extra: &str) -> Vec<Field> {
    crate::gff::fields(extra)
}

#[cfg(test)]
mod tests {
    use genie_core::{Feature, Region, Strand};

    use super::{fields, is_name, location, read, sniff};
    use crate::{Error, Reason};

    const VIRUS: &str = "\
LOCUS       NC_000001                120 bp    DNA     linear   VRL 01-JAN-2020
DEFINITION  A test virus.
ACCESSION   NC_000001
VERSION     NC_000001.2
FEATURES             Location/Qualifiers
     source          1..120
                     /organism=\"Test virus\"
     gene            10..60
                     /gene=\"abc\"
                     /locus_tag=\"T_001\"
     CDS             10..60
                     /gene=\"abc\"
                     /locus_tag=\"T_001\"
                     /product=\"abc protein with a
                     long name\"
                     /protein_id=\"YP_1.1\"
                     /translation=\"MKVLA
                     AAKV\"
     gene            complement(70..110)
                     /locus_tag=\"T_002\"
                     /db_xref=\"GeneID:7\"
     CDS             complement(join(70..80,
                     90..110))
                     /locus_tag=\"T_002\"
                     /note=\"say \"\"hi\"\"\"
     misc_feature    100^101
                     /note=\"a site\"
     repeat_region   <1..>5
ORIGIN
        1 acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt
       61 acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt
//
";

    const GENE: &str = "\
LOCUS       chrT                     300 bp    DNA     linear   CON 01-JAN-2020
ACCESSION   chrT
VERSION     .
FEATURES             Location/Qualifiers
     gene            1..300
                     /gene=\"GENE1\"
     mRNA            join(1..50,101..150,201..300)
                     /gene=\"GENE1\"
                     /transcript_id=\"NM_1.1\"
     mRNA            join(1..50,201..300)
                     /gene=\"GENE1\"
                     /transcript_id=\"NM_2.1\"
     CDS             join(31..50,201..240)
                     /gene=\"GENE1\"
                     /protein_id=\"NP_2.1\"
     CDS             join(21..50,101..150,201..230)
                     /gene=\"GENE1\"
                     /protein_id=\"NP_1.1\"
     ncRNA           complement(250..280)
                     /gene=\"GENE1-AS\"
ORIGIN
        1 aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa
       61 aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa
      121 aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa
      181 aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa
      241 aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa
//
";

    const PLASMID: &str = "\
LOCUS       pTest        100 bp DNA     circular SYN 01-JAN-1980
ACCESSION   .
VERSION     .
FEATURES             Location/Qualifiers
     rep_origin      complement(join(90..100,1..10))
                     /label=ori
                     /note=\"color: #ffef86; direction: LEFT\"
     CDS             20..40
                     /label=bla
                     /ApEinfo_fwdcolor=#7eff74
                     /ApEinfo_revcolor=#ff0000
     promoter        50..60
                     /label=\"lac promoter\"
                     /colour=\"0 0 255\"
ORIGIN
        1 gattacagat tacagattac agattacaga ttacagatta cagattacag attacagatt
       61 acagattaca gattacagat tacagattac agattacaga
//
";

    const EMBL: &str = "\
ID   X56734; SV 1; linear; mRNA; STD; PLN; 30 BP.
XX
AC   X56734; S46826;
XX
FH   Key             Location/Qualifiers
FH
FT   source          1..30
FT                   /organism=\"Trifolium repens\"
FT   CDS             5..25
FT                   /gene=\"tcg\"
FT                   /protein_id=\"CAA40058.1\"
XX
SQ   Sequence 30 BP; 8 A; 7 C; 7 G; 8 T; 0 other;
     aaacaaacca aatatggatt ttattgtagc                                  30
//
";

    fn summary(feature: &Feature) -> (String, Option<String>, Vec<Region>, Strand) {
        (
            feature.name().to_owned(),
            feature.id().map(str::to_owned),
            feature.blocks().to_vec(),
            feature.strand(),
        )
    }

    fn pairs(feature: &Feature) -> Vec<(String, String)> {
        fields(feature.extra())
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect()
    }

    fn pair(label: &str, value: &str) -> (String, String) {
        (label.to_owned(), value.to_owned())
    }

    fn span(start: u64, end: u64) -> Region {
        Region::between(start, end)
    }

    #[test]
    fn names_and_heads_tell_genbank_and_embl_files() {
        for name in [
            "a.gb",
            "a.GBK",
            "a.gbff.gz",
            "a.genbank",
            "a.embl",
            "a.emb.gz",
        ] {
            assert!(is_name(name), "{name}");
        }
        for name in ["a.gff", "a.bed", "a.fa", "gb"] {
            assert!(!is_name(name), "{name}");
        }
        assert!(sniff(b"LOCUS       NC_1"));
        assert!(sniff(b"ID   X56734; SV 1"));
        assert!(!sniff(b">chr1\nACGT"));
    }

    #[test]
    fn locations_give_parts_in_the_order_of_the_text() {
        assert_eq!(
            location("10..60"),
            Some((vec![span(9, 60)], Strand::Forward))
        );
        assert_eq!(location("7"), Some((vec![span(6, 7)], Strand::Forward)));
        assert_eq!(
            location("<1..>5"),
            Some((vec![span(0, 5)], Strand::Forward))
        );
        assert_eq!(
            location("100^101"),
            Some((vec![span(99, 101)], Strand::Forward))
        );
        assert_eq!(
            location("complement(join(70..80,90..110))"),
            Some((vec![span(69, 80), span(89, 110)], Strand::Reverse))
        );
        assert_eq!(
            location("join(complement(90..110),complement(70..80))"),
            Some((vec![span(89, 110), span(69, 80)], Strand::Reverse))
        );
        assert_eq!(
            location("order(1..5, 8..9)"),
            Some((vec![span(0, 5), span(7, 9)], Strand::Forward))
        );
        assert_eq!(
            location("join(J00194.1:100..202,1..20)"),
            Some((vec![span(0, 20)], Strand::Forward))
        );
        for bad in ["", "J00194.1:100..202", "10..x", "join(1..5", "60..10"] {
            assert_eq!(location(bad), None, "{bad}");
        }
    }

    #[test]
    fn genes_join_their_cds_and_other_features_stay_apart() {
        let records = read(VIRUS.as_bytes()).unwrap();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.name, "NC_000001.2");
        assert!(!record.circular);
        assert_eq!(record.length, 120);
        assert_eq!(record.bases.len(), 120);
        assert_eq!(&record.bases[..8], b"ACGTACGT");
        let found: Vec<_> = record.features.iter().map(summary).collect();
        assert_eq!(
            found,
            [
                (
                    "repeat_region".to_owned(),
                    None,
                    vec![span(0, 5)],
                    Strand::Forward
                ),
                (
                    "abc".to_owned(),
                    Some("YP_1.1".to_owned()),
                    vec![span(9, 60)],
                    Strand::Forward
                ),
                (
                    "T_002".to_owned(),
                    None,
                    vec![span(69, 80), span(89, 110)],
                    Strand::Reverse
                ),
                (
                    "a site".to_owned(),
                    None,
                    vec![span(99, 101)],
                    Strand::Forward
                ),
            ]
        );
        assert_eq!(record.features[1].coding(), Some(span(9, 60)));
        assert_eq!(
            pairs(&record.features[1]),
            [
                pair("type", "CDS"),
                pair("gene", "abc"),
                pair("locus_tag", "T_001"),
                pair("product", "abc protein with a long name"),
                pair("protein_id", "YP_1.1"),
            ]
        );
        assert_eq!(
            pairs(&record.features[2]),
            [
                pair("type", "CDS"),
                pair("locus_tag", "T_002"),
                pair("note", "say \"hi\""),
                pair("db_xref", "GeneID:7"),
            ]
        );
    }

    #[test]
    fn mrnas_take_the_cds_with_the_same_introns() {
        let records = read(GENE.as_bytes()).unwrap();
        let record = &records[0];
        assert_eq!(record.name, "chrT");
        let found: Vec<_> = record.features.iter().map(summary).collect();
        assert_eq!(
            found,
            [
                (
                    "GENE1".to_owned(),
                    Some("NM_1.1".to_owned()),
                    vec![span(0, 50), span(100, 150), span(200, 300)],
                    Strand::Forward
                ),
                (
                    "GENE1".to_owned(),
                    Some("NM_2.1".to_owned()),
                    vec![span(0, 50), span(200, 300)],
                    Strand::Forward
                ),
                (
                    "GENE1-AS".to_owned(),
                    None,
                    vec![span(249, 280)],
                    Strand::Reverse
                ),
            ]
        );
        let coding: Vec<Option<Region>> = record.features.iter().map(Feature::coding).collect();
        assert_eq!(coding, [Some(span(20, 230)), Some(span(30, 240)), None]);
        assert!(pairs(&record.features[0]).contains(&pair("protein_id", "NP_1.1")));
    }

    #[test]
    fn plasmids_keep_their_colors_and_split_at_the_origin() {
        let records = read(PLASMID.as_bytes()).unwrap();
        let record = &records[0];
        assert_eq!(record.name, "pTest");
        assert!(record.circular);
        let found: Vec<_> = record.features.iter().map(summary).collect();
        assert_eq!(
            found,
            [
                ("ori".to_owned(), None, vec![span(0, 10)], Strand::Reverse),
                ("bla".to_owned(), None, vec![span(19, 40)], Strand::Forward),
                (
                    "lac promoter".to_owned(),
                    None,
                    vec![span(49, 60)],
                    Strand::Forward
                ),
                ("ori".to_owned(), None, vec![span(89, 100)], Strand::Reverse),
            ]
        );
        let colors: Vec<Option<[u8; 3]>> = record.features.iter().map(Feature::color).collect();
        assert_eq!(
            colors,
            [
                Some([0xff, 0xef, 0x86]),
                Some([0x7e, 0xff, 0x74]),
                Some([0, 0, 255]),
                Some([0xff, 0xef, 0x86]),
            ]
        );
        assert_eq!(
            pairs(&record.features[0]),
            [pair("type", "rep_origin"), pair("label", "ori")]
        );
    }

    #[test]
    fn embl_records_name_themselves_by_accession_and_version() {
        let mut text = EMBL.to_owned();
        text.push_str(&VIRUS.replace("NC_000001.2", "NC_000009.1"));
        let records = read(text.as_bytes()).unwrap();
        let names: Vec<&str> = records.iter().map(|record| record.name.as_str()).collect();
        assert_eq!(names, ["X56734.1", "NC_000009.1"]);
        assert_eq!(records[0].bases.len(), 30);
        assert_eq!(&records[0].bases[..4], b"AAAC");
        let found: Vec<_> = records[0].features.iter().map(summary).collect();
        assert_eq!(
            found,
            [(
                "tcg".to_owned(),
                Some("CAA40058.1".to_owned()),
                vec![span(4, 25)],
                Strand::Forward
            )]
        );
    }

    #[test]
    fn other_files_are_not_genbank() {
        let error = read(">chr1\nACGT\n".as_bytes()).unwrap_err();
        assert!(
            matches!(error, Error::Invalid(Reason::NotGenBank)),
            "{error}"
        );
        assert_eq!(error.to_string(), "The file is not a GenBank or EMBL file.");
    }
}
