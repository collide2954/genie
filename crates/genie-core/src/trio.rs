//! Trios of a family: a child and its two parents among the samples of a file. Their calls tell
//! if the child can have its alleles from its parents.

use std::collections::HashMap;

use crate::samples::Table;
use crate::variant::Call;

/// The sex of a child, which tells what parent gives the X chromosome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sex {
    Unknown,
    Male,
    Female,
}

/// A child and its parents, as the places of their samples in a file, and the sex of the child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trio {
    pub child: usize,
    pub father: usize,
    pub mother: usize,
    pub sex: Sex,
}

/// What the calls of a trio say about the inheritance of a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inheritance {
    /// The child can have its alleles from its parents.
    Consistent,
    /// The child is heterozygous and both parents are homozygous reference: the child can have
    /// a new mutation. It is also a Mendelian error.
    DeNovo,
    /// Other calls that the child cannot have from its parents.
    Error,
}

impl Inheritance {
    pub fn word(self) -> &'static str {
        match self {
            Self::Consistent => CONSISTENT,
            Self::DeNovo => DE_NOVO,
            Self::Error => MENDELIAN_ERROR,
        }
    }
}

/// The names of the columns of a sample table that name the father of a sample, compared without
/// case.
const FATHERS: [&str; 4] = ["father", "paternal id", "paternal_id", "father_id"];
/// The names of the columns that name the mother of a sample.
const MOTHERS: [&str; 4] = ["mother", "maternal id", "maternal_id", "mother_id"];
/// The names of the columns that give the sex of a sample.
const SEXES: [&str; 2] = ["sex", "gender"];

/// The sex that a value of a sample table gives: the words, their first letters, or the PLINK
/// codes 1 and 2.
fn sex(value: Option<&str>) -> Sex {
    let Some(value) = value else {
        return Sex::Unknown;
    };
    let is = |words: &[&str]| words.iter().any(|word| value.eq_ignore_ascii_case(word));
    if is(&["male", "m", "1"]) {
        Sex::Male
    } else if is(&["female", "f", "2"]) {
        Sex::Female
    } else {
        Sex::Unknown
    }
}

/// The trios whose calls a contig can check: all trios on an autosome, only the trios of
/// daughters on X, and none on Y and on the mitochondria. A son has one X, from his mother, and
/// the mitochondria come from the mother.
pub fn on(contig: &str, trios: &[Trio]) -> Vec<Trio> {
    let name = contig
        .get(..3)
        .filter(|start| start.eq_ignore_ascii_case("chr"))
        .map_or(contig, |_| &contig[3..]);
    let is = |words: &[&str]| words.iter().any(|word| name.eq_ignore_ascii_case(word));
    if is(&["X"]) {
        trios
            .iter()
            .filter(|trio| trio.sex == Sex::Female)
            .copied()
            .collect()
    } else if is(&["Y", "M", "MT"]) {
        Vec::new()
    } else {
        trios.to_vec()
    }
}

/// The words for the kinds of inheritance, in the order that the field lists them.
pub const MENDELIAN_ERROR: &str = "Mendelian error";
pub const DE_NOVO: &str = "De novo";
pub const CONSISTENT: &str = "Consistent";

/// The inheritance of a variant in a trio, or `None` when a call is missing. A call is
/// homozygous reference, heterozygous or homozygous for an alternate allele. At a site with more
/// than one alternate allele, a heterozygous call can have two alternate alleles, so only the
/// calls that no alleles explain are errors there.
pub fn check(child: Call, father: Call, mother: Call, biallelic: bool) -> Option<Inheritance> {
    use Call::{Alternate, Het, Missing, Reference};
    if [child, father, mother].contains(&Missing) {
        return None;
    }
    Some(match child {
        Reference if father == Alternate || mother == Alternate => Inheritance::Error,
        Alternate if father == Reference || mother == Reference => Inheritance::Error,
        Het if father == Reference && mother == Reference => Inheritance::DeNovo,
        Het if biallelic && father == Alternate && mother == Alternate => Inheritance::Error,
        _ => Inheritance::Consistent,
    })
}

/// The trios among the samples of a file: each sample whose father and mother, as the sample
/// table names them, are samples of the file too.
pub fn trios(samples: &[String], table: &Table) -> Vec<Trio> {
    let column = |names: &[&str]| {
        table
            .columns
            .iter()
            .position(|column| names.iter().any(|name| column.eq_ignore_ascii_case(name)))
    };
    let (Some(fathers), Some(mothers)) = (column(&FATHERS), column(&MOTHERS)) else {
        return Vec::new();
    };
    let sexes = column(&SEXES);
    let places: HashMap<&str, usize> = samples
        .iter()
        .enumerate()
        .map(|(place, name)| (name.as_str(), place))
        .collect();
    let place = |name: &str| places.get(name).copied();
    samples
        .iter()
        .enumerate()
        .filter_map(|(child, name)| {
            Some(Trio {
                child,
                father: place(table.value(name, fathers)?)?,
                mother: place(table.value(name, mothers)?)?,
                sex: sex(sexes.and_then(|column| table.value(name, column))),
            })
        })
        .collect()
}

/// The kinds of inheritance of a variant in its trios, each once: the Mendelian errors first,
/// then de novo, then consistent. A de novo variant is a Mendelian error too.
pub fn words(calls: &[Call], trios: &[Trio], biallelic: bool) -> Vec<&'static str> {
    let call = |sample: usize| calls.get(sample).copied().unwrap_or(Call::Missing);
    let found: Vec<Inheritance> = trios
        .iter()
        .filter_map(|trio| {
            check(
                call(trio.child),
                call(trio.father),
                call(trio.mother),
                biallelic,
            )
        })
        .collect();
    let any = |kinds: &[Inheritance]| found.iter().any(|kind| kinds.contains(kind));
    [
        (
            MENDELIAN_ERROR,
            any(&[Inheritance::Error, Inheritance::DeNovo]),
        ),
        (DE_NOVO, any(&[Inheritance::DeNovo])),
        (CONSISTENT, any(&[Inheritance::Consistent])),
    ]
    .into_iter()
    .filter_map(|(word, found)| found.then_some(word))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::{Inheritance, Sex, Trio, check, on, trios, words};
    use crate::samples::Table;
    use crate::variant::Call::{Alternate, Het, Missing, Reference};

    #[test]
    fn children_get_one_allele_from_each_parent() {
        let consistent = Some(Inheritance::Consistent);
        assert_eq!(check(Reference, Het, Reference, true), consistent);
        assert_eq!(check(Het, Reference, Alternate, true), consistent);
        assert_eq!(check(Het, Het, Het, true), consistent);
        assert_eq!(check(Alternate, Het, Alternate, true), consistent);
        assert_eq!(
            check(Het, Reference, Reference, true),
            Some(Inheritance::DeNovo)
        );
        assert_eq!(
            check(Het, Reference, Reference, false),
            Some(Inheritance::DeNovo)
        );
        let error = Some(Inheritance::Error);
        assert_eq!(check(Reference, Alternate, Het, true), error);
        assert_eq!(check(Alternate, Het, Reference, true), error);
        assert_eq!(check(Alternate, Reference, Reference, true), error);
        assert_eq!(check(Het, Alternate, Alternate, true), error);
        assert_eq!(check(Het, Missing, Reference, true), None);
        assert_eq!(check(Missing, Het, Het, true), None);
    }

    #[test]
    fn multi_allelic_sites_flag_only_sure_errors() {
        assert_eq!(
            check(Het, Alternate, Alternate, false),
            Some(Inheritance::Consistent)
        );
        assert_eq!(
            check(Reference, Alternate, Het, false),
            Some(Inheritance::Error)
        );
        assert_eq!(
            check(Alternate, Reference, Het, false),
            Some(Inheritance::Error)
        );
    }

    fn names(samples: &[&str]) -> Vec<String> {
        samples.iter().map(|name| (*name).to_owned()).collect()
    }

    fn row(name: &str, values: &[&str]) -> (String, Vec<String>) {
        (name.to_owned(), names(values))
    }

    #[test]
    fn trios_need_both_parents_in_the_file() {
        let table = Table::new(
            names(&["Family", "Father", "Mother", "Sex"]),
            vec![
                row("kid", &["F1", "dad", "mum", "male"]),
                row("dad", &["F1", "", "", "male"]),
                row("mum", &["F1", "", "", "female"]),
                row("other", &["F2", "dad", "absent", "female"]),
            ],
        );
        let samples = names(&["mum", "kid", "other", "dad"]);
        assert_eq!(
            trios(&samples, &table),
            [Trio {
                child: 1,
                father: 3,
                mother: 0,
                sex: Sex::Male,
            }]
        );
        let header = Table::new(
            names(&["Paternal ID", "maternal_id", "Gender"]),
            vec![row("kid", &["dad", "mum", "2"])],
        );
        let found = trios(&samples, &header);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].sex, Sex::Female);
        let none = Table::new(names(&["Sex"]), vec![row("kid", &["male"])]);
        assert_eq!(trios(&samples, &none), []);
    }

    #[test]
    fn sex_chromosomes_check_only_the_trios_of_daughters() {
        let trio = |child, sex| Trio {
            child,
            father: 8,
            mother: 9,
            sex,
        };
        let trios = [
            trio(0, Sex::Female),
            trio(1, Sex::Male),
            trio(2, Sex::Unknown),
        ];
        assert_eq!(on("chr2", &trios), trios);
        assert_eq!(on("10", &trios), trios);
        assert_eq!(on("chrX", &trios), [trios[0]]);
        assert_eq!(on("X", &trios), [trios[0]]);
        for contig in ["chrY", "Y", "chrM", "MT"] {
            assert_eq!(on(contig, &trios), [], "{contig}");
        }
    }

    #[test]
    fn words_name_each_kind_once() {
        let first = Trio {
            child: 0,
            father: 1,
            mother: 2,
            sex: Sex::Unknown,
        };
        let second = Trio {
            child: 3,
            father: 1,
            mother: 2,
            sex: Sex::Unknown,
        };
        let both = [first, second];
        let calls = [Het, Reference, Reference, Reference];
        assert_eq!(
            words(&calls, &both, true),
            ["Mendelian error", "De novo", "Consistent"]
        );
        let calls = [Alternate, Reference, Het, Het];
        assert_eq!(
            words(&calls, &both, true),
            ["Mendelian error", "Consistent"]
        );
        let calls = [Het, Missing, Reference, Het];
        assert_eq!(words(&calls, &both, true), Vec::<&str>::new());
        assert_eq!(words(&calls, &[], true), Vec::<&str>::new());
    }
}
