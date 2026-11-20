//! Restriction enzymes: their sites, where they cut, and their cuts in a sequence.

use std::fmt;

use crate::search::{self, Pattern};
use crate::{Region, Strand};

/// Common enzymes, each with its site and ^ where it cuts the strand of the site.
pub const COMMON: [(&str, &str); 60] = [
    ("AatII", "GACGT^C"),
    ("AgeI", "A^CCGGT"),
    ("AluI", "AG^CT"),
    ("ApaI", "GGGCC^C"),
    ("AscI", "GG^CGCGCC"),
    ("AvrII", "C^CTAGG"),
    ("BamHI", "G^GATCC"),
    ("BfaI", "C^TAG"),
    ("BglII", "A^GATCT"),
    ("BsrGI", "T^GTACA"),
    ("BstBI", "TT^CGAA"),
    ("ClaI", "AT^CGAT"),
    ("CviQI", "G^TAC"),
    ("DdeI", "C^TNAG"),
    ("DpnII", "^GATC"),
    ("DraI", "TTT^AAA"),
    ("EagI", "C^GGCCG"),
    ("EcoRI", "G^AATTC"),
    ("EcoRV", "GAT^ATC"),
    ("FseI", "GGCCGG^CC"),
    ("HaeIII", "GG^CC"),
    ("HhaI", "GCG^C"),
    ("HindIII", "A^AGCTT"),
    ("HinfI", "G^ANTC"),
    ("HpaI", "GTT^AAC"),
    ("HpaII", "C^CGG"),
    ("KpnI", "GGTAC^C"),
    ("MboI", "^GATC"),
    ("MfeI", "C^AATTG"),
    ("MluI", "A^CGCGT"),
    ("MseI", "T^TAA"),
    ("MspI", "C^CGG"),
    ("NcoI", "C^CATGG"),
    ("NdeI", "CA^TATG"),
    ("NheI", "G^CTAGC"),
    ("NlaIII", "CATG^"),
    ("NotI", "GC^GGCCGC"),
    ("NsiI", "ATGCA^T"),
    ("PacI", "TTAAT^TAA"),
    ("PmeI", "GTTT^AAAC"),
    ("PstI", "CTGCA^G"),
    ("PvuI", "CGAT^CG"),
    ("PvuII", "CAG^CTG"),
    ("RsaI", "GT^AC"),
    ("SacI", "GAGCT^C"),
    ("SacII", "CCGC^GG"),
    ("SalI", "G^TCGAC"),
    ("Sau3AI", "^GATC"),
    ("SbfI", "CCTGCA^GG"),
    ("ScaI", "AGT^ACT"),
    ("SfiI", "GGCCNNNN^NGGCC"),
    ("SmaI", "CCC^GGG"),
    ("SpeI", "A^CTAGT"),
    ("SphI", "GCATG^C"),
    ("SspI", "AAT^ATT"),
    ("StuI", "AGG^CCT"),
    ("TaqI", "T^CGA"),
    ("XbaI", "T^CTAGA"),
    ("XhoI", "C^TCGAG"),
    ("XmaI", "C^CCGGG"),
];

pub const SHORTEST: usize = 4;
pub const LONGEST: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteError {
    Empty,
    Unknown(String),
    Short,
    Long,
    Cuts,
}

impl fmt::Display for SiteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "Type the name of an enzyme or its site."),
            Self::Unknown(name) => write!(
                f,
                "genie does not know the enzyme {name}. Type its site, for example G^AATTC."
            ),
            Self::Short => write!(f, "A site needs {SHORTEST} bases or more."),
            Self::Long => write!(f, "A site can have up to {LONGEST} bases."),
            Self::Cuts => write!(f, "Put one ^ at most in a site, at the cut."),
        }
    }
}

/// An enzyme: its name, the bases of its site in IUPAC codes, and the place of its cut in the
/// site, 0 before the first base. A site without ^ has no cut.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Enzyme {
    pub name: String,
    pub site: String,
    pub cut: Option<usize>,
}

impl Enzyme {
    /// An enzyme of the common list by its name in any letter case, or a site with ^ at the
    /// cut, for example G^AATTC. A typed site is its own name.
    pub fn parse(text: &str) -> Result<Self, SiteError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(SiteError::Empty);
        }
        if let Some((name, written)) = COMMON
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(text))
        {
            return site(name, written);
        }
        let letters: String = text
            .chars()
            .filter(|letter| !letter.is_whitespace())
            .collect::<String>()
            .to_ascii_uppercase();
        if !letters
            .bytes()
            .all(|byte| byte == b'^' || IUPAC.contains(&byte))
        {
            return Err(SiteError::Unknown(text.to_owned()));
        }
        site(&letters, &letters)
    }

    /// The site with ^ at the cut, for example G^AATTC.
    pub fn written(&self) -> String {
        match self.cut {
            Some(cut) => format!("{}^{}", &self.site[..cut], &self.site[cut..]),
            None => self.site.clone(),
        }
    }
}

const IUPAC: &[u8] = b"ACGTRYSWKMBDHVN";

fn site(name: &str, written: &str) -> Result<Enzyme, SiteError> {
    if written.matches('^').count() > 1 {
        return Err(SiteError::Cuts);
    }
    let site: String = written.chars().filter(|&letter| letter != '^').collect();
    if site.len() < SHORTEST {
        return Err(SiteError::Short);
    }
    if site.len() > LONGEST {
        return Err(SiteError::Long);
    }
    Ok(Enzyme {
        name: name.to_owned(),
        site,
        cut: written.find('^'),
    })
}

/// The common enzymes, in the order of their names.
pub fn common() -> Vec<Enzyme> {
    COMMON
        .iter()
        .filter_map(|(name, written)| site(name, written).ok())
        .collect()
}

/// A cut of an enzyme in a sequence: the place of the enzyme in its list, the site, the strand
/// of a site that is not the same on the two strands, and the place of the cut in the top
/// strand. The enzyme cuts before the base at `at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cut {
    pub enzyme: usize,
    pub site: Region,
    pub strand: Strand,
    pub at: Option<u64>,
}

impl Cut {
    /// The place of the cut, or the start of a site without a cut.
    pub fn place(&self) -> u64 {
        self.at.unwrap_or(self.site.start())
    }
}

/// The cuts of the enzymes in a sequence that starts at `start`, in the order of their sites.
pub fn cuts(enzymes: &[Enzyme], bases: &[u8], start: u64) -> Vec<Cut> {
    let mut found = Vec::new();
    for (index, enzyme) in enzymes.iter().enumerate() {
        let Ok(pattern) = Pattern::parse(&enzyme.site) else {
            continue;
        };
        for hit in search::find(&pattern, bases, start, 0, usize::MAX).hits {
            let at = enzyme.cut.map(|cut| match hit.strand {
                Strand::Reverse => hit.span.end() - cut as u64,
                Strand::Forward | Strand::Unknown => hit.span.start() + cut as u64,
            });
            found.push(Cut {
                enzyme: index,
                site: hit.span,
                strand: hit.strand,
                at,
            });
        }
    }
    found.sort_by_key(|cut| (cut.site.start(), cut.enzyme));
    found
}

/// The lengths of the fragments before and after each cut: to the nearest cut at a different
/// place on each side. A cut with no other cut on a side has no fragment there.
pub fn fragments(cuts: &[Cut]) -> Vec<(Option<u64>, Option<u64>)> {
    let mut places: Vec<u64> = cuts.iter().map(Cut::place).collect();
    places.sort_unstable();
    places.dedup();
    cuts.iter()
        .map(|cut| {
            let place = cut.place();
            let at = places.partition_point(|&other| other < place);
            let before = at.checked_sub(1).map(|index| place - places[index]);
            let after = places.get(at + 1).map(|&next| next - place);
            (before, after)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{COMMON, Cut, Enzyme, LONGEST, SiteError, common, cuts, fragments};
    use crate::{Region, Strand};

    fn enzyme(name: &str, site: &str, cut: Option<usize>) -> Enzyme {
        Enzyme {
            name: name.to_owned(),
            site: site.to_owned(),
            cut,
        }
    }

    #[test]
    fn the_common_enzymes_have_good_sites_and_names() {
        let enzymes = common();
        assert_eq!(enzymes.len(), COMMON.len());
        let names: HashSet<String> = enzymes.iter().map(|e| e.name.to_lowercase()).collect();
        assert_eq!(names.len(), COMMON.len());
        for (enzyme, (name, written)) in enzymes.iter().zip(COMMON) {
            assert_eq!(enzyme.name, name);
            assert_eq!(enzyme.written(), written);
            assert!(enzyme.cut.is_some_and(|cut| cut <= enzyme.site.len()));
        }
        assert_eq!(enzymes[17], enzyme("EcoRI", "GAATTC", Some(1)));
    }

    #[test]
    fn names_and_typed_sites_give_enzymes() {
        assert_eq!(
            Enzyme::parse(" ecori "),
            Ok(enzyme("EcoRI", "GAATTC", Some(1)))
        );
        assert_eq!(
            Enzyme::parse("g^aat tc"),
            Ok(enzyme("G^AATTC", "GAATTC", Some(1)))
        );
        assert_eq!(
            Enzyme::parse("GGTCTC"),
            Ok(enzyme("GGTCTC", "GGTCTC", None))
        );
        assert_eq!(Enzyme::parse("CATG^"), Ok(enzyme("CATG^", "CATG", Some(4))));
        assert_eq!(Enzyme::parse("  "), Err(SiteError::Empty));
        assert_eq!(
            Enzyme::parse("EcoRX"),
            Err(SiteError::Unknown("EcoRX".to_owned()))
        );
        assert_eq!(Enzyme::parse("G^A^ATTC"), Err(SiteError::Cuts));
        assert_eq!(Enzyme::parse("GA^T"), Err(SiteError::Short));
        assert_eq!(
            Enzyme::parse(&"A".repeat(LONGEST + 1)),
            Err(SiteError::Long)
        );
        assert_eq!(
            SiteError::Unknown("EcoRX".to_owned()).to_string(),
            "genie does not know the enzyme EcoRX. Type its site, for example G^AATTC."
        );
    }

    #[test]
    fn cuts_come_from_both_strands_in_the_order_of_their_sites() {
        let enzymes = [
            enzyme("EcoRI", "GAATTC", Some(1)),
            enzyme("GAC^C", "GACC", Some(3)),
            enzyme("GAAGAC", "GAAGAC", None),
        ];
        let found = cuts(&enzymes, b"AAGACCTTGGTCAAGAATTCAAGAAGAC", 100);
        assert_eq!(
            found,
            [
                Cut {
                    enzyme: 1,
                    site: Region::between(102, 106),
                    strand: Strand::Forward,
                    at: Some(105),
                },
                Cut {
                    enzyme: 1,
                    site: Region::between(108, 112),
                    strand: Strand::Reverse,
                    at: Some(109),
                },
                Cut {
                    enzyme: 0,
                    site: Region::between(114, 120),
                    strand: Strand::Unknown,
                    at: Some(115),
                },
                Cut {
                    enzyme: 2,
                    site: Region::between(122, 128),
                    strand: Strand::Forward,
                    at: None,
                },
            ]
        );
        assert_eq!(found[3].place(), 122);
    }

    #[test]
    fn iupac_sites_match_their_bases() {
        let hinf = [enzyme("HinfI", "GANTC", Some(1))];
        let places: Vec<Option<u64>> = cuts(&hinf, b"GAATCGACTCGANNC", 0)
            .iter()
            .map(|cut| cut.at)
            .collect();
        assert_eq!(places, [Some(1), Some(6)]);
    }

    #[test]
    fn fragments_go_to_the_nearest_cut_at_another_place() {
        let cut = |enzyme, at| Cut {
            enzyme,
            site: Region::between(at, at + 4),
            strand: Strand::Unknown,
            at: Some(at),
        };
        let list = [cut(0, 10), cut(0, 30), cut(1, 30), cut(1, 70)];
        assert_eq!(
            fragments(&list),
            [
                (None, Some(20)),
                (Some(20), Some(40)),
                (Some(20), Some(40)),
                (Some(40), None)
            ]
        );
        assert_eq!(fragments(&[]), []);
    }
}
