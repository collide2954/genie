use std::cmp::Ordering;

use crate::Region;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    Missing,
    Reference,
    Het,
    Alternate,
}

impl Call {
    pub fn parse(genotype: &str) -> Self {
        let genotype = genotype.split(':').next().unwrap_or_default();
        let (mut reference, mut alternate) = (false, false);
        for allele in genotype.split(['/', '|']) {
            match allele {
                "0" => reference = true,
                "." | "" => {}
                _ => alternate = true,
            }
        }
        match (reference, alternate) {
            (false, false) => Self::Missing,
            (true, false) => Self::Reference,
            (false, true) => Self::Alternate,
            (true, true) => Self::Het,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Snv(u8),
    Insertion,
    Deletion,
    Complex,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub id: String,
    pub span: Region,
    pub reference: String,
    pub alternates: Vec<String>,
    pub quality: Option<f32>,
    pub filter: String,
    pub info: String,
    pub calls: Vec<Call>,
}

impl Variant {
    pub fn kind(&self) -> Kind {
        let Some(alternate) = self.alternates.first() else {
            return Kind::Complex;
        };
        if let Some(symbol) = alternate.strip_prefix('<') {
            return if symbol.starts_with("DEL") {
                Kind::Deletion
            } else if symbol.starts_with("INS") {
                Kind::Insertion
            } else {
                Kind::Complex
            };
        }
        if matches!(alternate.as_str(), "." | "*") || alternate.contains(['[', ']']) {
            return Kind::Complex;
        }
        match self.reference.len().cmp(&alternate.len()) {
            Ordering::Equal if alternate.len() == 1 => {
                Kind::Snv(alternate.as_bytes()[0].to_ascii_uppercase())
            }
            Ordering::Equal => Kind::Complex,
            Ordering::Less => Kind::Insertion,
            Ordering::Greater => Kind::Deletion,
        }
    }

    pub fn passes(&self) -> bool {
        matches!(self.filter.as_str(), "PASS" | "." | "")
    }
}

#[cfg(test)]
mod tests {
    use super::{Call, Kind, Variant};
    use crate::Region;

    fn variant(reference: &str, alternates: &[&str]) -> Variant {
        Variant {
            id: String::new(),
            span: Region::between(100, 100 + reference.len() as u64),
            reference: reference.to_owned(),
            alternates: alternates.iter().map(|alt| (*alt).to_owned()).collect(),
            quality: None,
            filter: "PASS".to_owned(),
            info: String::new(),
            calls: Vec::new(),
        }
    }

    #[test]
    fn kinds_follow_the_first_alternate() {
        assert_eq!(variant("A", &["G"]).kind(), Kind::Snv(b'G'));
        assert_eq!(variant("a", &["t", "C"]).kind(), Kind::Snv(b'T'));
        assert_eq!(variant("A", &["ACGT"]).kind(), Kind::Insertion);
        assert_eq!(variant("ACGT", &["A"]).kind(), Kind::Deletion);
        assert_eq!(variant("AC", &["GT"]).kind(), Kind::Complex);
        assert_eq!(variant("A", &["<DEL>"]).kind(), Kind::Deletion);
        assert_eq!(variant("A", &["<INS:ME>"]).kind(), Kind::Insertion);
        assert_eq!(variant("A", &["<DUP>"]).kind(), Kind::Complex);
        assert_eq!(variant("A", &["."]).kind(), Kind::Complex);
    }

    #[test]
    fn pass_and_missing_filters_pass() {
        let mut site = variant("A", &["G"]);
        assert!(site.passes());
        site.filter = ".".to_owned();
        assert!(site.passes());
        site.filter = "LowQual".to_owned();
        assert!(!site.passes());
    }

    #[test]
    fn calls_read_genotypes() {
        let calls: Vec<Call> = [
            "0/0",
            "0|1",
            "1/0",
            "1/1",
            "1|2",
            "./.",
            ".",
            "1",
            "0",
            "./1",
            "0/0:30:12",
        ]
        .iter()
        .map(|genotype| Call::parse(genotype))
        .collect();
        assert_eq!(
            calls,
            [
                Call::Reference,
                Call::Het,
                Call::Het,
                Call::Alternate,
                Call::Alternate,
                Call::Missing,
                Call::Missing,
                Call::Alternate,
                Call::Reference,
                Call::Alternate,
                Call::Reference,
            ]
        );
    }
}
