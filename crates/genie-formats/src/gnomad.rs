//! Allele frequencies from the gnomAD API: one question for one variant, with the counts of the
//! exomes, of the genomes and of each genetic ancestry group.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::http::{self, Url};
use crate::json::{self, Json};

/// The address of the gnomAD API.
pub const API: &str = "https://gnomad.broadinstitute.org/api";
/// The most questions that genie asks gnomAD in a minute. gnomAD allows about 10.
const IN_A_MINUTE: usize = 8;
const MINUTE: Duration = Duration::from_secs(60);
/// The genetic ancestry groups of gnomAD that a card shows, with their names.
const GROUPS: [(&str, &str); 11] = [
    ("afr", "African/African American"),
    ("amr", "Admixed American"),
    ("asj", "Ashkenazi Jewish"),
    ("eas", "East Asian"),
    ("fin", "European (Finnish)"),
    ("mid", "Middle Eastern"),
    ("nfe", "European (non-Finnish)"),
    ("sas", "South Asian"),
    ("ami", "Amish"),
    ("remaining", "Remaining"),
    ("oth", "Remaining"),
];

/// The gnomAD data set of a genome and its name: v4.1 for hg38, v2.1.1 for hg19.
pub fn dataset(genome: &str) -> Option<(&'static str, &'static str)> {
    match genome {
        "hg38" => Some(("gnomad_r4", "gnomAD v4.1")),
        "hg19" => Some(("gnomad_r2_1", "gnomAD v2.1.1")),
        _ => None,
    }
}

fn bases(allele: &str) -> bool {
    !allele.is_empty()
        && allele
            .bytes()
            .all(|base| matches!(base.to_ascii_uppercase(), b'A' | b'C' | b'G' | b'T'))
}

/// The gnomAD name of a variant, as `1-55051215-G-GA`: the chromosome without "chr", the
/// position of the first base from 1, and the two alleles without the bases at their ends that
/// they share, as gnomAD writes them. None for a contig that gnomAD does not have, or for an
/// allele that is not bases.
pub fn variant_id(contig: &str, position: u64, reference: &str, alternate: &str) -> Option<String> {
    let chromosome = contig.strip_prefix("chr").unwrap_or(contig);
    let numbered = chromosome
        .parse::<u8>()
        .is_ok_and(|number| (1..=22).contains(&number));
    if !(numbered || chromosome == "X" || chromosome == "Y") {
        return None;
    }
    if !bases(reference) || !bases(alternate) {
        return None;
    }
    let (mut reference, mut alternate) = (
        reference.to_ascii_uppercase().into_bytes(),
        alternate.to_ascii_uppercase().into_bytes(),
    );
    while reference.len() > 1 && alternate.len() > 1 && reference.last() == alternate.last() {
        reference.pop();
        alternate.pop();
    }
    let mut position = position;
    while reference.len() > 1 && alternate.len() > 1 && reference[0] == alternate[0] {
        reference.remove(0);
        alternate.remove(0);
        position += 1;
    }
    let (reference, alternate) = (
        String::from_utf8_lossy(&reference),
        String::from_utf8_lossy(&alternate),
    );
    Some(format!("{chromosome}-{position}-{reference}-{alternate}"))
}

/// The body of the question about a variant to the gnomAD API.
pub fn question(id: &str, dataset: &str) -> String {
    let counts = "ac an ac_hom populations { id ac an }";
    let query = format!(
        "query($id: String!, $dataset: DatasetId!) {{ variant(variantId: $id, dataset: $dataset) \
         {{ variant_id exome {{ {counts} }} genome {{ {counts} }} }} }}"
    );
    format!(r#"{{"query":"{query}","variables":{{"id":"{id}","dataset":"{dataset}"}}}}"#)
}

/// The alleles of a group of samples: the alternate alleles and all alleles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Count {
    pub alternate: u64,
    pub total: u64,
}

impl Count {
    /// The share of alternate alleles, from 0 to 1. None without alleles.
    pub fn frequency(self) -> Option<f64> {
        (self.total > 0).then(|| self.alternate as f64 / self.total as f64)
    }

    fn add(self, other: Self) -> Self {
        Self {
            alternate: self.alternate + other.alternate,
            total: self.total + other.total,
        }
    }
}

/// The frequencies of a variant in gnomAD, with the exomes and the genomes together.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Frequencies {
    pub all: Count,
    /// The samples with two alternate alleles.
    pub homozygotes: u64,
    /// The counts of each genetic ancestry group, by its name, in the order of `GROUPS`.
    pub groups: Vec<(&'static str, Count)>,
}

fn number(value: Option<&Json>) -> u64 {
    match value {
        Some(Json::Number(number)) if *number >= 0.0 => *number as u64,
        _ => 0,
    }
}

/// The answer of the gnomAD API about a variant: its frequencies, None when gnomAD does not have
/// the variant, or why the answer is not valid.
pub fn answer(text: &str) -> Result<Option<Frequencies>, String> {
    let answer = json::parse(text)?;
    let variant = answer.get("data").and_then(|data| data.get("variant"));
    let Some(variant) = variant.filter(|variant| !matches!(variant, Json::Null)) else {
        let message = answer
            .get("errors")
            .and_then(Json::list)
            .and_then(<[Json]>::first)
            .and_then(|error| error.get("message"))
            .and_then(Json::text);
        return match message {
            Some(message) if !message.to_ascii_lowercase().contains("not found") => {
                Err(format!("gnomAD says: {message}"))
            }
            _ => Ok(None),
        };
    };
    let mut frequencies = Frequencies::default();
    let mut groups = [Count::default(); GROUPS.len()];
    for part in ["exome", "genome"] {
        let Some(counts) = variant.get(part).filter(|part| !matches!(part, Json::Null)) else {
            continue;
        };
        frequencies.all = frequencies.all.add(Count {
            alternate: number(counts.get("ac")),
            total: number(counts.get("an")),
        });
        frequencies.homozygotes += number(counts.get("ac_hom"));
        for group in counts
            .get("populations")
            .and_then(Json::list)
            .unwrap_or(&[])
        {
            let id = group.get("id").and_then(Json::text).unwrap_or_default();
            if let Some(at) = GROUPS.iter().position(|(known, _)| *known == id) {
                groups[at] = groups[at].add(Count {
                    alternate: number(group.get("ac")),
                    total: number(group.get("an")),
                });
            }
        }
    }
    frequencies.groups = GROUPS
        .iter()
        .zip(groups)
        .filter(|(_, count)| count.total > 0)
        .map(|((_, name), count)| (*name, count))
        .collect();
    Ok(Some(frequencies))
}

/// A share as a percent with three digits, for example `0.0123 %`, `1.23 %` or `100 %`.
pub fn percent(share: f64) -> String {
    let value = share * 100.0;
    if value <= 0.0 {
        return "0 %".to_owned();
    }
    let digits = (2 - value.log10().floor() as i32).clamp(0, 8) as usize;
    format!("{value:.digits$} %")
}

/// The times of the last questions to gnomAD.
static ASKED: Mutex<VecDeque<Instant>> = Mutex::new(VecDeque::new());

/// Waits until a question to gnomAD keeps to the questions that gnomAD allows in a minute.
fn wait_for_turn() {
    loop {
        let wait = {
            let mut asked = ASKED.lock().unwrap_or_else(PoisonError::into_inner);
            let now = Instant::now();
            while asked
                .front()
                .is_some_and(|time| now.duration_since(*time) >= MINUTE)
            {
                asked.pop_front();
            }
            if asked.len() < IN_A_MINUTE {
                asked.push_back(now);
                return;
            }
            asked.front().map_or(Duration::ZERO, |first| {
                MINUTE.saturating_sub(now.duration_since(*first))
            })
        };
        std::thread::sleep(wait);
    }
}

/// Asks a gnomAD API at an address about a variant of a data set.
pub fn ask_at(url: &Url, id: &str, dataset: &str) -> Result<Option<Frequencies>, String> {
    wait_for_turn();
    let body = http::post(url, "application/json", question(id, dataset).as_bytes())
        .map_err(|error| error.to_string())?;
    answer(&String::from_utf8_lossy(&body))
}

/// Asks the gnomAD API about a variant of a data set.
pub fn ask(id: &str, dataset: &str) -> Result<Option<Frequencies>, String> {
    let url = Url::parse(API).map_err(|error| error.to_string())?;
    ask_at(&url, id, dataset)
}

#[cfg(test)]
mod tests {
    use super::{Count, answer, ask, ask_at, dataset, percent, question, variant_id};
    use crate::http::Url;
    use crate::json;
    use crate::server::serve;

    /// The real gnomAD API: APOE rs429358 on hg38 and on hg19, and an insertion that gnomAD
    /// does not have.
    #[test]
    #[ignore = "asks the real gnomAD API"]
    fn real_gnomad_answers() {
        for (id, dataset) in [
            ("19-44908684-T-C", "gnomad_r4"),
            ("19-45411941-T-C", "gnomad_r2_1"),
            ("1-100000-C-CGATTACAGATTACA", "gnomad_r4"),
        ] {
            let found = ask(id, dataset);
            println!("{id} {dataset}: {found:?}");
            let found = found.unwrap();
            if id.starts_with("1-") {
                assert_eq!(found, None);
            } else {
                let found = found.unwrap();
                assert!(found.all.total > 100_000, "{found:?}");
                assert!(!found.groups.is_empty(), "{found:?}");
            }
        }
    }

    const ANSWER: &str = r#"{"data":{"variant":{"variant_id":"19-44908684-T-C",
        "exome":{"ac":100,"an":1000,"ac_hom":4,"populations":[
            {"id":"afr","ac":30,"an":100},{"id":"afr_XX","ac":10,"an":50},
            {"id":"nfe","ac":70,"an":900},{"id":"XX","ac":50,"an":500}]},
        "genome":{"ac":20,"an":200,"ac_hom":1,"populations":[
            {"id":"afr","ac":20,"an":100},{"id":"remaining","ac":0,"an":100}]}}}}"#;

    #[test]
    fn variants_get_the_names_of_gnomad() {
        assert_eq!(
            variant_id("chr1", 55_051_215, "g", "GA").as_deref(),
            Some("1-55051215-G-GA")
        );
        assert_eq!(
            variant_id("X", 100, "CTT", "CT").as_deref(),
            Some("X-100-CT-C"),
            "shared bases at the end go"
        );
        assert_eq!(
            variant_id("chr2", 100, "AC", "AG").as_deref(),
            Some("2-101-C-G"),
            "shared bases at the start go"
        );
        for (contig, alternate) in [
            ("chrM", "T"),
            ("chrUn_x", "T"),
            ("chr1", "<DEL>"),
            ("chr1", "*"),
        ] {
            assert_eq!(
                variant_id(contig, 1, "A", alternate),
                None,
                "{contig} {alternate}"
            );
        }
        assert_eq!(dataset("hg38"), Some(("gnomad_r4", "gnomAD v4.1")));
        assert_eq!(dataset("mm39"), None);
    }

    #[test]
    fn questions_are_json_with_the_variant_and_the_data_set() {
        let body = question("1-55051215-G-GA", "gnomad_r4");
        let parsed = json::parse(&body).unwrap();
        let variables = parsed.get("variables").unwrap();
        assert_eq!(
            variables.get("id").and_then(json::Json::text).as_deref(),
            Some("1-55051215-G-GA")
        );
        let query = parsed.get("query").and_then(json::Json::text).unwrap();
        assert!(
            query.contains("variant(variantId: $id, dataset: $dataset)"),
            "{query}"
        );
        assert!(
            query.contains("genome { ac an ac_hom populations { id ac an } }"),
            "{query}"
        );
    }

    #[test]
    fn answers_add_the_exomes_and_the_genomes() {
        let found = answer(ANSWER).unwrap().unwrap();
        assert_eq!(
            found.all,
            Count {
                alternate: 120,
                total: 1_200
            }
        );
        assert_eq!(found.homozygotes, 5);
        assert_eq!(
            found.groups,
            [
                (
                    "African/African American",
                    Count {
                        alternate: 50,
                        total: 200
                    }
                ),
                (
                    "European (non-Finnish)",
                    Count {
                        alternate: 70,
                        total: 900
                    }
                ),
                (
                    "Remaining",
                    Count {
                        alternate: 0,
                        total: 100
                    }
                ),
            ]
        );
        let missing = r#"{"errors":[{"message":"Variant not found"}],"data":{"variant":null}}"#;
        assert_eq!(answer(missing), Ok(None));
        let wrong = r#"{"errors":[{"message":"Unknown dataset"}],"data":null}"#;
        assert_eq!(
            answer(wrong),
            Err("gnomAD says: Unknown dataset".to_owned())
        );
        assert!(answer("<html>").is_err());
    }

    #[test]
    fn percents_keep_three_digits() {
        let shown: Vec<String> = [0.0, 0.000_123, 0.012_34, 0.5, 1.0]
            .into_iter()
            .map(percent)
            .collect();
        assert_eq!(shown, ["0 %", "0.0123 %", "1.23 %", "50.0 %", "100 %"]);
        assert_eq!(Count::default().frequency(), None);
    }

    #[test]
    fn the_api_answers_a_posted_question() {
        let server = serve(vec![("/api".to_owned(), ANSWER.as_bytes().to_vec())]);
        let url = Url::parse(&server.url("/api")).unwrap();
        let found = ask_at(&url, "19-44908684-T-C", "gnomad_r4")
            .unwrap()
            .unwrap();
        assert_eq!(found.all.total, 1_200);
        assert_eq!(server.requests(), 1);
    }
}
