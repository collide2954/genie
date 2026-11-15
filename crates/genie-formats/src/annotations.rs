//! The annotations that VEP (CSQ), `SnpEff` (ANN) and bcftools csq (BCSQ) write into the INFO
//! of VCF records: one item for each allele and transcript, with the columns that the header
//! names.

use std::borrow::Cow;

use genie_core::variant::{Annotation, Annotations, Impact};

use crate::fields;

/// The number of annotations that a variant keeps for its card, the most severe first.
const KEPT: usize = 3;

/// Consequences without "_variant", by impact as the Ensembl VEP table gives them. The others
/// are modifiers.
const HIGH: [&str; 10] = [
    "transcript_ablation",
    "splice_acceptor",
    "splice_donor",
    "stop_gained",
    "frameshift",
    "stop_lost",
    "start_lost",
    "transcript_amplification",
    "feature_elongation",
    "feature_truncation",
];
const MODERATE: [&str; 5] = [
    "inframe_insertion",
    "inframe_deletion",
    "missense",
    "protein_altering",
    "inframe_altering",
];
const LOW: [&str; 8] = [
    "splice_donor_5th_base",
    "splice_region",
    "splice_donor_region",
    "splice_polypyrimidine_tract",
    "incomplete_terminal_codon",
    "start_retained",
    "stop_retained",
    "synonymous",
];

/// The columns of an INFO field that holds annotations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    id: String,
    names: Vec<String>,
    consequence: usize,
    allele: Option<usize>,
    impact: Option<usize>,
    gene: Option<usize>,
    transcript: Option<usize>,
    biotype: Option<usize>,
    coding: Option<usize>,
    protein: Option<usize>,
    /// The columns that mark the transcript that the tool chose, for example CANONICAL.
    chosen: Vec<usize>,
}

/// The layouts of the INFO fields that hold annotations, from the INFO lines of a header.
pub fn layouts(infos: &[(String, String)]) -> Vec<Layout> {
    infos
        .iter()
        .filter_map(|(id, about)| Layout::new(id, about))
        .collect()
}

impl Layout {
    /// The layout of an INFO field whose description lists its columns, as in "Format:
    /// Allele|Consequence|…" (VEP, bcftools) or "Functional annotations: 'Allele | Annotation
    /// | …'" (`SnpEff`). A field without a column of consequences holds no annotations.
    fn new(id: &str, description: &str) -> Option<Self> {
        let start = description
            .find("Format:")
            .map(|at| at + "Format:".len())
            .or_else(|| description.find(": '").map(|at| at + 3))?;
        let list = description[start..].trim().trim_matches(['\'', '"']).trim();
        if !list.contains('|') {
            return None;
        }
        let names: Vec<String> = list.split('|').map(|name| name.trim().to_owned()).collect();
        let column = |wanted: &[&str]| {
            wanted
                .iter()
                .find_map(|name| names.iter().position(|found| found == name))
        };
        Some(Self {
            id: id.to_owned(),
            consequence: column(&["Consequence", "Annotation"])?,
            allele: column(&["Allele"]),
            impact: column(&["IMPACT", "Annotation_Impact"]),
            gene: column(&["SYMBOL", "Gene_Name", "gene"]),
            transcript: column(&["Feature", "Feature_ID", "transcript"]),
            biotype: column(&["BIOTYPE", "Transcript_BioType", "biotype"]),
            coding: column(&["HGVSc", "HGVS.c"]),
            protein: column(&["HGVSp", "HGVS.p", "amino_acid_change"]),
            chosen: ["CANONICAL", "MANE_SELECT", "PICK"]
                .iter()
                .filter_map(|name| column(&[name]))
                .collect(),
            names,
        })
    }

    /// The columns that the annotation itself shows.
    fn shown(&self) -> [Option<usize>; 8] {
        [
            Some(self.consequence),
            self.allele,
            self.impact,
            self.gene,
            self.transcript,
            self.biotype,
            self.coding,
            self.protein,
        ]
    }

    fn read<'a>(&'a self, value: &'a str, found: &mut Vec<Found<'a>>) {
        for item in value.split(',') {
            let item = item.trim();
            if item.is_empty() || item.starts_with('@') {
                continue;
            }
            let columns: Vec<&str> = item.split('|').map(str::trim).collect();
            let get = |column: Option<usize>| {
                column
                    .and_then(|at| columns.get(at))
                    .copied()
                    .unwrap_or_default()
            };
            let consequences: Vec<String> = get(Some(self.consequence))
                .trim_start_matches('*')
                .split('&')
                .filter(|term| !term.is_empty())
                .map(str::to_owned)
                .collect();
            if consequences.is_empty() {
                continue;
            }
            let impact = impact(get(self.impact)).unwrap_or_else(|| {
                consequences
                    .iter()
                    .map(|term| impact_of(term))
                    .max()
                    .unwrap_or_default()
            });
            let chosen = self.chosen.iter().any(|at| {
                columns
                    .get(*at)
                    .is_some_and(|value| !matches!(*value, "" | "0" | "NO" | "no"))
            });
            let annotation = Annotation {
                allele: get(self.allele).to_owned(),
                consequences,
                impact,
                gene: get(self.gene).to_owned(),
                transcript: get(self.transcript).to_owned(),
                biotype: get(self.biotype).to_owned(),
                coding: change(get(self.coding)),
                protein: change(get(self.protein)),
            };
            found.push(Found {
                layout: self,
                columns,
                chosen,
                annotation,
            });
        }
    }
}

/// An annotation with its columns, while genie sorts the annotations of a record.
struct Found<'a> {
    layout: &'a Layout,
    columns: Vec<&'a str>,
    chosen: bool,
    annotation: Annotation,
}

/// The annotations in the INFO of a record, and the INFO without the fields that held them.
pub fn split<'a>(info: &'a str, layouts: &[Layout]) -> (Annotations, Cow<'a, str>) {
    let holds = |key: &str| layouts.iter().find(|layout| layout.id == key);
    if layouts.is_empty()
        || !info
            .split(';')
            .any(|entry| holds(entry.split('=').next().unwrap_or_default()).is_some())
    {
        return (Annotations::default(), Cow::Borrowed(info));
    }
    let mut found = Vec::new();
    let mut rest = Vec::new();
    for entry in info.split(';') {
        let (key, value) = entry.split_once('=').unwrap_or((entry, ""));
        match holds(key) {
            Some(layout) => layout.read(value, &mut found),
            None => rest.push(entry),
        }
    }
    found.sort_by(|one, other| {
        other
            .annotation
            .impact
            .cmp(&one.annotation.impact)
            .then(other.chosen.cmp(&one.chosen))
    });
    let mut fields = summary(&found);
    if let Some(first) = found.first() {
        fields.extend(columns(first));
    }
    let others = found.len().saturating_sub(KEPT);
    let first = found
        .into_iter()
        .take(KEPT)
        .map(|found| found.annotation)
        .collect();
    let annotations = Annotations {
        first,
        others,
        fields,
    };
    (annotations, Cow::Owned(rest.join(";")))
}

/// Fields with the consequences, the impacts, the genes and the biotypes of all annotations,
/// the most severe first.
fn summary(found: &[Found]) -> Vec<(String, String)> {
    let mut lists: [(&str, Vec<&str>); 4] = [
        ("Consequence", Vec::new()),
        ("Impact", Vec::new()),
        ("Gene", Vec::new()),
        ("Biotype", Vec::new()),
    ];
    for entry in found {
        let annotation = &entry.annotation;
        let values = [
            annotation.consequences.iter().map(String::as_str).collect(),
            vec![annotation.impact.word()],
            vec![annotation.gene.as_str()],
            vec![annotation.biotype.as_str()],
        ];
        for ((_, list), values) in lists.iter_mut().zip(values) {
            for value in values {
                if !value.is_empty() && !list.contains(&value) {
                    list.push(value);
                }
            }
        }
    }
    lists
        .into_iter()
        .filter(|(_, list)| !list.is_empty())
        .map(|(label, list)| (label.to_owned(), list.join(", ")))
        .collect()
}

/// The other columns of an annotation, for example SIFT, as fields named after the INFO field.
fn columns(found: &Found) -> Vec<(String, String)> {
    let shown = found.layout.shown();
    found
        .layout
        .names
        .iter()
        .zip(&found.columns)
        .enumerate()
        .filter(|(at, (_, value))| !shown.contains(&Some(*at)) && fields::shown(value))
        .map(|(_, (name, value))| {
            (
                format!("{} {name}", found.layout.id),
                unescaped(value).replace('&', ", "),
            )
        })
        .collect()
}

fn impact(text: &str) -> Option<Impact> {
    match text.to_ascii_uppercase().as_str() {
        "HIGH" => Some(Impact::High),
        "MODERATE" => Some(Impact::Moderate),
        "LOW" => Some(Impact::Low),
        "MODIFIER" => Some(Impact::Modifier),
        _ => None,
    }
}

/// The impact of a consequence, for files that do not give it, as from bcftools csq.
fn impact_of(term: &str) -> Impact {
    let term = term.to_ascii_lowercase();
    let term = term.strip_suffix("_variant").unwrap_or(&term);
    if HIGH.contains(&term) {
        Impact::High
    } else if MODERATE.contains(&term) {
        Impact::Moderate
    } else if LOW.contains(&term) {
        Impact::Low
    } else {
        Impact::Modifier
    }
}

/// A change without the name of its transcript or protein, and with its %XX escapes decoded:
/// "ENSP00000350283.3:p.Arg1699Trp" gives "p.Arg1699Trp".
fn change(text: &str) -> String {
    let change = text.split_once(':').map_or(text, |(_, change)| change);
    unescaped(change)
}

/// A value with its %XX escapes decoded, as VEP writes "=" in "p.%3D".
fn unescaped(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut found = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let value = bytes
            .get(index + 1..index + 3)
            .filter(|hex| bytes[index] == b'%' && hex.iter().all(u8::is_ascii_hexdigit))
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        if let Some(value) = value {
            found.push(value);
            index += 3;
        } else {
            found.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(found).unwrap_or_else(|_| text.to_owned())
}

#[cfg(test)]
mod tests {
    use genie_core::variant::{Annotation, Annotations, Impact};

    use super::{layouts, split};

    const VEP: &str = "Consequence annotations from Ensembl VEP. Format: Allele|Consequence|IMPACT|\
        SYMBOL|Gene|Feature_type|Feature|BIOTYPE|EXON|INTRON|HGVSc|HGVSp|CANONICAL|SIFT";
    const SNPEFF: &str = "Functional annotations: 'Allele | Annotation | Annotation_Impact | \
        Gene_Name | Gene_ID | Feature_Type | Feature_ID | Transcript_BioType | Rank | HGVS.c | \
        HGVS.p | cDNA.pos / cDNA.length | CDS.pos / CDS.length | AA.pos / AA.length | Distance | \
        ERRORS / WARNINGS / INFO' ";
    const BCSQ: &str = "Haplotype-aware consequence annotation from BCFtools/csq, see \
        http://samtools.github.io/bcftools/howtos/csq-calling.html for details. Format: \
        Consequence|gene|transcript|biotype|strand|amino_acid_change|dna_change";
    const LOF: &str = "Predicted loss of function effects for this variant. Format: 'Gene_Name \
        | Gene_ID | Number_of_transcripts_in_gene | Percent_of_transcripts_affected'";

    fn infos(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(id, about)| ((*id).to_owned(), (*about).to_owned()))
            .collect()
    }

    fn pairs(fields: &[(String, String)]) -> Vec<(&str, &str)> {
        fields
            .iter()
            .map(|(label, value)| (label.as_str(), value.as_str()))
            .collect()
    }

    #[test]
    fn vep_annotations_come_out_of_the_info_most_severe_first() {
        let layouts = layouts(&infos(&[("DP", "Depth"), ("CSQ", VEP)]));
        assert_eq!(layouts.len(), 1);
        let info = "DP=10;CSQ=\
            T|intron_variant&non_coding_transcript_variant|MODIFIER|BRCA1|ENSG00000012048|\
            Transcript|ENST00000461574.1|retained_intron||5/6|ENST00000461574.1:n.300-12G>A|||,\
            T|missense_variant|MODERATE|BRCA1|ENSG00000012048|Transcript|ENST00000357654.9|\
            protein_coding|16/23||ENST00000357654.9:c.5095C>T|ENSP00000350283.3:p.Arg1699Trp|YES|\
            deleterious(0),\
            T|upstream_gene_variant|MODIFIER|NBR2|ENSG00000198496|Transcript|ENST00000589174.5|\
            lncRNA||||||,\
            T|regulatory_region_variant|MODIFIER|||RegulatoryFeature|ENSR00000000001|promoter||||||;\
            AF=0.5";
        let (annotations, rest) = split(info, &layouts);
        assert_eq!(rest, "DP=10;AF=0.5");
        assert_eq!(
            annotations.first[0],
            Annotation {
                allele: "T".to_owned(),
                consequences: vec!["missense_variant".to_owned()],
                impact: Impact::Moderate,
                gene: "BRCA1".to_owned(),
                transcript: "ENST00000357654.9".to_owned(),
                biotype: "protein_coding".to_owned(),
                coding: "c.5095C>T".to_owned(),
                protein: "p.Arg1699Trp".to_owned(),
            }
        );
        let genes: Vec<&str> = annotations
            .first
            .iter()
            .map(|annotation| annotation.transcript.as_str())
            .collect();
        assert_eq!(
            genes,
            [
                "ENST00000357654.9",
                "ENST00000461574.1",
                "ENST00000589174.5"
            ]
        );
        assert_eq!(annotations.first[1].coding, "n.300-12G>A");
        assert_eq!(annotations.others, 1);
        assert_eq!(
            pairs(&annotations.fields),
            [
                (
                    "Consequence",
                    "missense_variant, intron_variant, non_coding_transcript_variant, \
                     upstream_gene_variant, regulatory_region_variant"
                ),
                ("Impact", "MODERATE, MODIFIER"),
                ("Gene", "BRCA1, NBR2"),
                (
                    "Biotype",
                    "protein_coding, retained_intron, lncRNA, promoter"
                ),
                ("CSQ Gene", "ENSG00000012048"),
                ("CSQ Feature_type", "Transcript"),
                ("CSQ EXON", "16/23"),
                ("CSQ CANONICAL", "YES"),
                ("CSQ SIFT", "deleterious(0)"),
            ]
        );
    }

    #[test]
    fn snpeff_annotations_give_their_impact() {
        let layouts = layouts(&infos(&[("ANN", SNPEFF), ("LOF", LOF)]));
        assert_eq!(layouts.len(), 1);
        let info = "ANN=\
            A|synonymous_variant|LOW|TP53|ENSG00000141510|transcript|ENST00000413465.6|\
            protein_coding|5/7|c.639C>T|p.Arg213Arg|||||,\
            A|stop_gained|HIGH|TP53|ENSG00000141510|transcript|ENST00000269305.9|\
            protein_coding|6/11|c.637C>T|p.Arg213*|829/2512|637/1182|213/393||;\
            LOF=(TP53|ENSG00000141510|17|0.12)";
        let (annotations, rest) = split(info, &layouts);
        assert_eq!(rest, "LOF=(TP53|ENSG00000141510|17|0.12)");
        let first = &annotations.first[0];
        assert_eq!(
            (first.impact, first.coding.as_str(), first.protein.as_str()),
            (Impact::High, "c.637C>T", "p.Arg213*")
        );
        assert_eq!(annotations.first[1].impact, Impact::Low);
        assert_eq!(annotations.others, 0);
        assert!(pairs(&annotations.fields).contains(&("ANN CDS.pos / CDS.length", "637/1182")));
        assert!(pairs(&annotations.fields).contains(&("Impact", "HIGH, LOW")));
    }

    #[test]
    fn bcftools_consequences_get_the_impact_of_their_terms() {
        let layouts = layouts(&infos(&[("BCSQ", BCSQ)]));
        let info = "BCSQ=@12828200,\
            *synonymous|PRAMEF12|ENST00000357726|protein_coding|+|8L|12828124G>A,\
            missense|PRAMEF12|ENST00000357726|protein_coding|+|7I>7M|12828121C>G";
        let (annotations, rest) = split(info, &layouts);
        assert_eq!(rest, "");
        let first = &annotations.first[0];
        assert_eq!(
            (
                first.consequences.as_slice(),
                first.impact,
                first.gene.as_str(),
                first.protein.as_str()
            ),
            (
                &["missense".to_owned()][..],
                Impact::Moderate,
                "PRAMEF12",
                "7I>7M"
            )
        );
        assert_eq!(annotations.first[1].impact, Impact::Low);
        assert_eq!(
            pairs(&annotations.fields)[4..],
            [("BCSQ strand", "+"), ("BCSQ dna_change", "12828121C>G")]
        );
    }

    #[test]
    fn records_without_annotations_keep_their_info() {
        let layouts = layouts(&infos(&[("CSQ", VEP)]));
        let (annotations, rest) = split("DP=3;AF=0.1", &layouts);
        assert_eq!(annotations, Annotations::default());
        assert_eq!(rest, "DP=3;AF=0.1");
        let (annotations, rest) = split("DP=3;CSQ=T|%3D|", &[]);
        assert_eq!(annotations, Annotations::default());
        assert_eq!(rest, "DP=3;CSQ=T|%3D|");
    }

    #[test]
    fn escapes_in_changes_are_decoded() {
        let layouts = layouts(&infos(&[("CSQ", VEP)]));
        let info = "CSQ=C|synonymous_variant|LOW|GENE|ENSG1|Transcript|ENST1|protein_coding|\
            2/9||ENST1:c.21T>C|ENSP1:p.Leu7%3D||";
        let (annotations, _) = split(info, &layouts);
        assert_eq!(annotations.first[0].protein, "p.Leu7=");
    }
}
