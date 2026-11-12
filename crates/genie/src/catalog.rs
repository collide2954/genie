const UCSC: &str = "https://hgdownload.soe.ucsc.edu";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Genes,
    Variation,
    Conservation,
    Regulation,
}

impl Group {
    pub const ALL: [Self; 4] = [
        Self::Genes,
        Self::Variation,
        Self::Conservation,
        Self::Regulation,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Genes => "Genes",
            Self::Variation => "Variation",
            Self::Conservation => "Conservation",
            Self::Regulation => "Regulation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub group: Group,
    pub name: &'static str,
    pub about: &'static str,
    pub path: &'static str,
    pub label: Option<&'static str>,
}

impl Entry {
    pub fn url(&self) -> String {
        format!("{UCSC}/{}", self.path)
    }

    const fn labeled(mut self, field: &'static str) -> Self {
        self.label = Some(field);
        self
    }
}

const fn entry(group: Group, name: &'static str, about: &'static str, path: &'static str) -> Entry {
    Entry {
        group,
        name,
        about,
        path,
        label: None,
    }
}

const CLINVAR: &str = "Variants in ClinVar, colored by clinical significance.";
const DBSNP: &str = "dbSNP variants with a frequency of 1 % or more in a large population.";
const EVA: &str = "Variants from the European Variation Archive, release 6.";
const GC: &str = "Percent of G and C bases in 5-base windows.";
const CCRES: &str = "Candidate cis-regulatory elements from ENCODE, colored by class.";

const HG38: &[Entry] = &[
    entry(
        Group::Genes,
        "MANE Select",
        "One matched RefSeq and Ensembl transcript for each protein-coding gene.",
        "gbdb/hg38/mane/mane.bb",
    ),
    entry(
        Group::Genes,
        "GENCODE V48",
        "All GENCODE V48 transcripts.",
        "gbdb/hg38/gencode/gencodeV48.bb",
    ),
    entry(
        Group::Variation,
        "ClinVar",
        CLINVAR,
        "gbdb/hg38/bbi/clinvar/clinvarMain.bb",
    ),
    entry(
        Group::Variation,
        "dbSNP 155 common",
        DBSNP,
        "gbdb/hg38/snp/dbSnp155Common.bb",
    ),
    entry(
        Group::Conservation,
        "phyloP 100-way",
        "Conservation of each base in 100 vertebrates.",
        "goldenPath/hg38/phyloP100way/hg38.phyloP100way.bw",
    ),
    entry(
        Group::Conservation,
        "phastCons 100-way",
        "Chance that each base is in a conserved element, 100 vertebrates.",
        "goldenPath/hg38/phastCons100way/hg38.phastCons100way.bw",
    ),
    entry(
        Group::Conservation,
        "GC percent",
        GC,
        "gbdb/hg38/bbi/gc5BaseBw/gc5Base.bw",
    ),
    entry(
        Group::Regulation,
        "ENCODE cCREs",
        CCRES,
        "gbdb/hg38/encode3/ccre/encodeCcreCombined.bb",
    ),
];

const HG19: &[Entry] = &[
    entry(
        Group::Genes,
        "GENCODE V48lift37",
        "All GENCODE V48 transcripts, mapped to GRCh37.",
        "gbdb/hg19/gencode/gencodeV48lift37.bb",
    ),
    entry(
        Group::Variation,
        "ClinVar",
        CLINVAR,
        "gbdb/hg19/bbi/clinvar/clinvarMain.bb",
    ),
    entry(
        Group::Variation,
        "dbSNP 155 common",
        DBSNP,
        "gbdb/hg19/snp/dbSnp155Common.bb",
    ),
    entry(
        Group::Conservation,
        "phyloP 100-way",
        "Conservation of each base in 100 vertebrates.",
        "goldenPath/hg19/phyloP100way/hg19.100way.phyloP100way.bw",
    ),
    entry(
        Group::Conservation,
        "phastCons 100-way",
        "Chance that each base is in a conserved element, 100 vertebrates.",
        "goldenPath/hg19/phastCons100way/hg19.100way.phastCons.bw",
    ),
];

const MM39: &[Entry] = &[
    entry(
        Group::Genes,
        "GENCODE VM37",
        "All GENCODE VM37 transcripts.",
        "gbdb/mm39/gencode/gencodeVM37.bb",
    ),
    entry(
        Group::Variation,
        "EVA SNP 6",
        EVA,
        "gbdb/mm39/bbi/evaSnp6.bb",
    ),
    entry(
        Group::Conservation,
        "phyloP 35-way",
        "Conservation of each base in 35 vertebrates.",
        "goldenPath/mm39/phyloP35way/mm39.phyloP35way.bw",
    ),
    entry(
        Group::Conservation,
        "phastCons 35-way",
        "Chance that each base is in a conserved element, 35 vertebrates.",
        "goldenPath/mm39/phastCons35way/mm39.phastCons35way.bw",
    ),
    entry(
        Group::Conservation,
        "GC percent",
        GC,
        "gbdb/mm39/bbi/gc5BaseBw/gc5Base.bw",
    ),
];

const MM10: &[Entry] = &[
    entry(
        Group::Genes,
        "UCSC Genes",
        "UCSC Known Genes transcripts, based on GENCODE.",
        "gbdb/mm10/knownGene.bb",
    )
    .labeled("geneName"),
    entry(
        Group::Variation,
        "EVA SNP 6",
        EVA,
        "gbdb/mm10/bbi/evaSnp6.bb",
    ),
    entry(
        Group::Conservation,
        "phyloP 60-way",
        "Conservation of each base in 60 vertebrates.",
        "goldenPath/mm10/phyloP60way/mm10.60way.phyloP60way.bw",
    ),
    entry(
        Group::Conservation,
        "phastCons 60-way",
        "Chance that each base is in a conserved element, 60 vertebrates.",
        "goldenPath/mm10/phastCons60way/mm10.60way.phastCons.bw",
    ),
    entry(
        Group::Conservation,
        "GC percent",
        GC,
        "gbdb/mm10/bbi/gc5Base.bw",
    ),
    entry(
        Group::Regulation,
        "ENCODE cCREs",
        CCRES,
        "gbdb/mm10/encode3/ccre/encodeCcreCombined.bb",
    ),
];

const HS1: &[Entry] = &[entry(
    Group::Conservation,
    "GC percent",
    GC,
    "hubs/GCA/009/914/755/GCA_009914755.4/bbi/GCA_009914755.4_T2T-CHM13v2.0.gc5Base.bw",
)];

const RN7: &[Entry] = &[entry(
    Group::Conservation,
    "GC percent",
    GC,
    "gbdb/rn7/bbi/gc5BaseBw/gc5Base.bw",
)];

const DANRER11: &[Entry] = &[entry(
    Group::Conservation,
    "GC percent",
    GC,
    "gbdb/danRer11/bbi/gc5BaseBw/gc5Base.bw",
)];

const DM6: &[Entry] = &[
    entry(
        Group::Conservation,
        "phyloP 124-way",
        "Conservation of each base in 124 insects.",
        "goldenPath/dm6/phyloP124way/dm6.phyloP124way.bw",
    ),
    entry(
        Group::Conservation,
        "GC percent",
        GC,
        "gbdb/dm6/bbi/gc5BaseBw/gc5Base.bw",
    ),
];

const CE11: &[Entry] = &[
    entry(
        Group::Conservation,
        "phyloP 135-way",
        "Conservation of each base in 135 nematodes.",
        "goldenPath/ce11/phyloP135way/ce11.phyloP135way.bw",
    ),
    entry(
        Group::Conservation,
        "GC percent",
        GC,
        "gbdb/ce11/bbi/gc5BaseBw/gc5Base.bw",
    ),
];

pub fn entries(genome: &str) -> &'static [Entry] {
    match genome {
        "hg38" => HG38,
        "hg19" => HG19,
        "mm39" => MM39,
        "mm10" => MM10,
        "hs1" => HS1,
        "rn7" => RN7,
        "danRer11" => DANRER11,
        "dm6" => DM6,
        "ce11" => CE11,
        _ => &[],
    }
}

const LISTS: [&[Entry]; 9] = [HG38, HG19, MM39, MM10, HS1, RN7, DANRER11, DM6, CE11];

pub fn label(url: &str) -> Option<&'static str> {
    LISTS
        .iter()
        .flat_map(|list| list.iter())
        .find(|entry| entry.url() == url)
        .and_then(|entry| entry.label)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{Group, entries, label};
    use crate::genome;

    fn groups(genome: &str) -> Vec<Group> {
        let mut found: Vec<Group> = Vec::new();
        for entry in entries(genome) {
            if !found.contains(&entry.group) {
                found.push(entry.group);
            }
        }
        found
    }

    #[test]
    fn human_and_mouse_hold_their_groups() {
        assert_eq!(groups("hg38"), Group::ALL);
        assert_eq!(
            groups("hg19"),
            [Group::Genes, Group::Variation, Group::Conservation]
        );
        assert_eq!(
            groups("mm39"),
            [Group::Genes, Group::Variation, Group::Conservation]
        );
        assert_eq!(groups("mm10"), Group::ALL);
        for other in ["hs1", "rn7", "danRer11", "dm6", "ce11"] {
            assert_eq!(groups(other), [Group::Conservation], "{other}");
        }
        assert_eq!(entries("sacCer3").len(), 0);
        assert_eq!(entries("nope").len(), 0);
    }

    #[test]
    fn some_entries_name_their_label_field() {
        let url = |genome: &str, name: &str| {
            entries(genome)
                .iter()
                .find(|entry| entry.name == name)
                .unwrap()
                .url()
        };
        assert_eq!(label(&url("mm10", "UCSC Genes")), Some("geneName"));
        assert_eq!(label(&url("hg38", "MANE Select")), None);
        assert_eq!(label("https://example.org/gbdb/mm10/knownGene.bb"), None);
    }

    #[test]
    fn entries_point_at_ucsc_files() {
        for genome in genome::all() {
            let mut names = HashSet::new();
            for entry in entries(&genome.id) {
                let url = entry.url();
                assert!(url.starts_with("https://hgdownload.soe.ucsc.edu/"), "{url}");
                let kind = std::path::Path::new(entry.path)
                    .extension()
                    .and_then(|kind| kind.to_str());
                assert!(matches!(kind, Some("bb" | "bw")), "{url}");
                assert!(names.insert(entry.name), "{} twice", entry.name);
                assert!(entry.about.ends_with('.'), "{}", entry.about);
            }
        }
    }
}
