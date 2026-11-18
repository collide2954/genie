#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Test {
    Keep(String),
    Hide(String),
    AtLeast(f64),
    AtMost(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub label: String,
    pub test: Test,
}

impl Rule {
    pub fn passes(&self, fields: &[Field]) -> bool {
        let value = fields
            .iter()
            .find(|field| field.label == self.label)
            .map(|field| field.value.as_str());
        let mut found = value.into_iter().flat_map(items);
        match &self.test {
            Test::Keep(wanted) => found.any(|item| item == wanted),
            Test::Hide(unwanted) => !found.any(|item| item == unwanted),
            Test::AtLeast(bound) => found.filter_map(number).any(|number| number >= *bound),
            Test::AtMost(bound) => found.filter_map(number).any(|number| number <= *bound),
        }
    }
}

pub fn passes(rules: &[Rule], fields: &[Field]) -> bool {
    rules.iter().all(|rule| rule.passes(fields))
}

pub fn items(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
}

pub fn number(item: &str) -> Option<f64> {
    item.trim()
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

/// What a card shows for an item of a list that has no value.
pub const MISSING: &str = "–";

/// True for an item of a list that has no value: an empty item, or the `-inf` or `nan` that UCSC
/// files give when a project does not report a number.
fn missing(item: &str) -> bool {
    ["", "-inf", "nan", "-nan"]
        .iter()
        .any(|word| item.eq_ignore_ascii_case(word))
}

/// The items of a list as a card shows them: each item in its place, [`MISSING`] for an item
/// without a value, and no empty item after a last comma. A list without values gives `None`.
pub fn listed(value: &str) -> Option<String> {
    let mut items: Vec<&str> = value.split(',').map(str::trim).collect();
    if items.len() > 1 && items.last() == Some(&"") {
        items.pop();
    }
    if items.iter().all(|item| missing(item)) {
        return None;
    }
    let shown: Vec<&str> = items
        .into_iter()
        .map(|item| if missing(item) { MISSING } else { item })
        .collect();
    Some(shown.join(", "))
}

/// The Sequence Ontology terms of variants by their numbers: the consequences of Ensembl VEP, the
/// functional classes of dbSNP and the variant types of `ClinVar`.
const ONTOLOGY: &[(u32, &str)] = &[
    (159, "deletion"),
    (199, "translocation"),
    (289, "microsatellite"),
    (667, "insertion"),
    (1019, "copy_number_variation"),
    (1059, "sequence_alteration"),
    (1060, "sequence_variant"),
    (1483, "SNV"),
    (1566, "regulatory_region_variant"),
    (1567, "stop_retained_variant"),
    (1574, "splice_acceptor_variant"),
    (1575, "splice_donor_variant"),
    (1578, "stop_lost"),
    (1580, "coding_sequence_variant"),
    (1582, "initiator_codon_variant"),
    (1583, "missense_variant"),
    (1587, "stop_gained"),
    (1589, "frameshift_variant"),
    (1590, "terminator_codon_variant"),
    (1619, "non_coding_transcript_variant"),
    (1620, "mature_miRNA_variant"),
    (1621, "NMD_transcript_variant"),
    (1623, "5_prime_UTR_variant"),
    (1624, "3_prime_UTR_variant"),
    (1626, "incomplete_terminal_codon_variant"),
    (1627, "intron_variant"),
    (1628, "intergenic_variant"),
    (1630, "splice_region_variant"),
    (1631, "upstream_gene_variant"),
    (1632, "downstream_gene_variant"),
    (1634, "500B_downstream_variant"),
    (1636, "2KB_upstream_variant"),
    (1742, "copy_number_gain"),
    (1743, "copy_number_loss"),
    (1782, "TF_binding_site_variant"),
    (1787, "splice_donor_5th_base_variant"),
    (1792, "non_coding_transcript_exon_variant"),
    (1818, "protein_altering_variant"),
    (1819, "synonymous_variant"),
    (1820, "inframe_indel"),
    (1821, "inframe_insertion"),
    (1822, "inframe_deletion"),
    (1889, "transcript_amplification"),
    (1891, "regulatory_region_amplification"),
    (1892, "TFBS_amplification"),
    (1893, "transcript_ablation"),
    (1894, "regulatory_region_ablation"),
    (1895, "TFBS_ablation"),
    (1906, "feature_truncation"),
    (1907, "feature_elongation"),
    (1968, "coding_transcript_variant"),
    (2007, "MNV"),
    (2012, "start_lost"),
    (2019, "start_retained_variant"),
    (2152, "genic_downstream_transcript_variant"),
    (2153, "genic_upstream_transcript_variant"),
    (2169, "splice_polypyrimidine_tract_variant"),
    (2170, "splice_donor_region_variant"),
    (1_000_032, "delins"),
    (1_000_035, "duplication"),
    (1_000_036, "inversion"),
];

/// True for the description of a field with Sequence Ontology numbers.
pub fn names_ontology(about: &str) -> bool {
    about.to_ascii_lowercase().contains("sequence ontology")
}

/// The Sequence Ontology term of a number such as `1623` or `SO:0001623`, as
/// `5_prime_UTR_variant (SO:0001623)`. An unknown number gives only its id, and other values stay
/// as they are. 0 is the number of no term and gives `None`.
pub fn ontology(value: &str) -> Option<String> {
    let value = value.trim();
    let digits = value.strip_prefix("SO:").unwrap_or(value);
    let number = digits
        .parse::<u32>()
        .ok()
        .filter(|_| digits.bytes().all(|byte| byte.is_ascii_digit()));
    let Some(number) = number else {
        return Some(value.to_owned());
    };
    if number == 0 {
        return None;
    }
    let id = format!("SO:{number:07}");
    Some(match ONTOLOGY.iter().find(|(known, _)| *known == number) {
        Some((_, term)) => format!("{term} ({id})"),
        None => id,
    })
}

const LABEL_LIMIT: usize = 40;
const STOPS: [&str; 8] = ["(", "{", "[", ",", ";", ":", ". ", "|"];
const ENTITIES: [(&str, &str); 6] = [
    ("&lt;", "<"),
    ("&gt;", ">"),
    ("&quot;", "\""),
    ("&#39;", "'"),
    ("&nbsp;", " "),
    ("&amp;", "&"),
];

pub fn label(description: &str, name: &str) -> String {
    let text = description.trim().trim_matches('"').trim();
    let end = STOPS
        .iter()
        .filter_map(|stop| text.find(stop))
        .min()
        .unwrap_or(text.len());
    let text = text[..end].trim();
    let mut characters = text.chars();
    let Some(first) = characters.next() else {
        return name.to_owned();
    };
    let word = text.split_whitespace().next().unwrap_or_default();
    let label: String = if word.chars().any(char::is_uppercase) {
        text.to_owned()
    } else {
        first.to_uppercase().chain(characters).collect()
    };
    if label.chars().count() <= LABEL_LIMIT {
        return label;
    }
    let mut short = String::new();
    for word in label.split(' ') {
        let length = short.chars().count() + usize::from(!short.is_empty()) + word.chars().count();
        if length >= LABEL_LIMIT {
            break;
        }
        if !short.is_empty() {
            short.push(' ');
        }
        short.push_str(word);
    }
    if short.is_empty() {
        short = label.chars().take(LABEL_LIMIT - 1).collect();
    }
    short.push('…');
    short
}

pub fn linked(value: &str) -> &str {
    match value.split_once('|') {
        Some((id, text))
            if !id.is_empty()
                && !text.is_empty()
                && id.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            text
        }
        _ => value,
    }
}

fn numeric(text: &str) -> String {
    let mut found = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("&#") {
        found.push_str(&rest[..at]);
        let tail = &rest[at + 2..];
        let decoded = tail.find(';').and_then(|end| {
            let digits = &tail[..end];
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => digits.parse().ok(),
            }?;
            Some((char::from_u32(code)?, end))
        });
        if let Some((character, end)) = decoded {
            found.push(character);
            rest = &tail[end + 1..];
        } else {
            found.push_str("&#");
            rest = tail;
        }
    }
    found.push_str(rest);
    found
}

pub fn shown(value: &str) -> bool {
    !matches!(value.trim(), "" | "none" | ".")
}

pub fn plain(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        let opens = character == '<'
            && characters
                .peek()
                .is_some_and(|next| next.is_ascii_alphabetic() || matches!(next, '/' | '!'));
        if opens {
            characters.by_ref().find(|skipped| *skipped == '>');
            continue;
        }
        text.push(character);
    }
    let text = ENTITIES
        .iter()
        .fold(numeric(&text), |text, (entity, character)| {
            text.replace(entity, character)
        });
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        Field, Rule, Test, items, label, linked, listed, names_ontology, number, ontology, passes,
        plain, shown,
    };

    fn fields(pairs: &[(&str, &str)]) -> Vec<Field> {
        pairs
            .iter()
            .map(|(label, value)| Field {
                label: (*label).to_owned(),
                value: (*value).to_owned(),
            })
            .collect()
    }

    fn rule(label: &str, test: Test) -> Rule {
        Rule {
            label: label.to_owned(),
            test,
        }
    }

    #[test]
    fn rules_keep_hide_and_bound_any_item() {
        let gene = fields(&[
            ("Tags", "CCDS, basic"),
            ("Gene type", "protein_coding"),
            ("Frequency", "0.42,-inf,0.08"),
        ]);
        let other = fields(&[
            ("Tags", "MANE_Select"),
            ("Gene type", "lncRNA"),
            ("Frequency", "0.01,-inf,x"),
        ]);
        let keep = rule("Tags", Test::Keep("basic".to_owned()));
        assert!(keep.passes(&gene));
        assert!(!keep.passes(&other));
        assert!(!keep.passes(&[]));
        let hide = rule("Gene type", Test::Hide("lncRNA".to_owned()));
        assert!(hide.passes(&gene));
        assert!(!hide.passes(&other));
        assert!(hide.passes(&[]));
        let least = rule("Frequency", Test::AtLeast(0.05));
        assert!(least.passes(&gene));
        assert!(!least.passes(&other));
        assert!(!least.passes(&[]));
        let most = rule("Frequency", Test::AtMost(0.01));
        assert!(!most.passes(&gene));
        assert!(most.passes(&other));
        assert!(passes(&[keep.clone(), hide], &gene));
        assert!(!passes(&[keep, least], &other));
        assert!(passes(&[], &other));
    }

    #[test]
    fn values_split_into_items_and_numbers() {
        assert_eq!(items(" a, ,b ").collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(number("0.5"), Some(0.5));
        assert_eq!(number(" 30 "), Some(30.0));
        assert_eq!(number("-inf"), None);
        assert_eq!(number("NaN"), None);
        assert_eq!(number("x"), None);
    }

    #[test]
    fn list_items_keep_their_places() {
        let listed = |value| listed(value).unwrap();
        assert_eq!(
            listed("0.0329473,0.0111098,-inf,0.5,"),
            "0.0329473, 0.0111098, –, 0.5"
        );
        assert_eq!(listed("G,G,,G,"), "G, G, –, G");
        assert_eq!(listed(",T,"), "–, T");
        assert_eq!(listed("NaN, 1 ,-nan"), "–, 1, –");
        assert_eq!(listed("T,"), "T");
        assert_eq!(listed("G"), "G");
        assert_eq!(listed("inf,-INF"), "inf, –");
    }

    #[test]
    fn lists_without_values_are_not_shown() {
        assert_eq!(listed("-inf,-inf,"), None);
        assert_eq!(listed(",,,"), None);
        assert_eq!(listed("-inf"), None);
        assert_eq!(listed(""), None);
    }

    #[test]
    fn ontology_numbers_show_their_terms() {
        assert_eq!(
            ontology("1623").as_deref(),
            Some("5_prime_UTR_variant (SO:0001623)")
        );
        assert_eq!(
            ontology("SO:0001583").as_deref(),
            Some("missense_variant (SO:0001583)")
        );
        assert_eq!(ontology("1000032").as_deref(), Some("delins (SO:1000032)"));
        assert_eq!(ontology(" 99 ").as_deref(), Some("SO:0000099"));
        assert_eq!(ontology("0"), None);
        assert_eq!(ontology("SO:0000000"), None);
        assert_eq!(
            ontology("intron_variant").as_deref(),
            Some("intron_variant")
        );
        assert_eq!(
            ontology("SO:0001583|missense_variant").as_deref(),
            Some("SO:0001583|missense_variant")
        );
        assert!(names_ontology(
            "\"Sequence Ontology (SO) ID of greatest functional impact on gene\""
        ));
        assert!(names_ontology("Sequence Ontology id for variant type"));
        assert!(!names_ontology("Variation class/type"));
    }

    #[test]
    fn labels_are_short_descriptions() {
        assert_eq!(
            label("\"Clinical significance\"", "clinSign"),
            "Clinical significance"
        );
        assert_eq!(
            label(
                "Status of CDS start annotation (none, unknown, incomplete, or complete)",
                "cdsStartStat"
            ),
            "Status of CDS start annotation"
        );
        assert_eq!(
            label(
                "Exon frame {0,1,2}, or -1 if no frame for exon",
                "exonFrames"
            ),
            "Exon frame"
        );
        assert_eq!(
            label("support level, tsl1 is strongest support", "level"),
            "Support level"
        );
        assert_eq!(
            label(
                "Consequence annotations from Ensembl VEP. Format: Allele|Consequence",
                "CSQ"
            ),
            "Consequence annotations from Ensembl VEP"
        );
        assert_eq!(
            label("Allele frequency: 0.5 is half", "AF"),
            "Allele frequency"
        );
        assert_eq!(label("dbSNP membership", "DB"), "dbSNP membership");
        assert_eq!(label("mRNA count", "N"), "mRNA count");
        assert_eq!(label("", "zScore"), "zScore");
        assert_eq!(label("\"\"", "zScore"), "zScore");
        assert_eq!(
            label(
                "A description that goes on and on without any stop at all",
                "long"
            ),
            "A description that goes on and on…"
        );
        assert_eq!(
            label(
                "VCF position|Can differ from ClinVar position (see VCF)",
                "vcfDesc"
            ),
            "VCF position"
        );
        assert_eq!(
            label(
                "Number of projects reporting frequencies for this variant",
                "freqSourceCount"
            ),
            "Number of projects reporting…"
        );
        assert_eq!(
            label("Pneumonoultramicroscopicsilicovolcanoconiosis", "x"),
            "Pneumonoultramicroscopicsilicovolcanoco…"
        );
    }

    #[test]
    fn link_values_keep_their_text() {
        assert_eq!(linked("2597|GAPDH"), "GAPDH");
        assert_eq!(
            linked("4388052|VCV004388052 : NM_002046.7(GAPDH):c.-23-1G>T"),
            "VCV004388052 : NM_002046.7(GAPDH):c.-23-1G>T"
        );
        assert_eq!(linked("Sarcoma|Melanoma"), "Sarcoma|Melanoma");
        assert_eq!(linked("|x"), "|x");
        assert_eq!(linked("12|"), "12|");
        assert_eq!(linked("plain"), "plain");
    }

    #[test]
    fn empty_none_and_dot_values_are_not_shown() {
        assert!(shown("Pathogenic"));
        assert!(shown("0"));
        assert!(!shown(""));
        assert!(!shown("  "));
        assert!(!shown("none"));
        assert!(!shown("."));
    }

    #[test]
    fn values_lose_html_tags() {
        assert_eq!(
            plain("<b>Promoter</b>-like &amp; <a href=\"x\">CTCF</a>"),
            "Promoter-like & CTCF"
        );
        assert_eq!(plain("  a < b  "), "a < b");
        assert_eq!(plain("plain"), "plain");
        assert_eq!(
            plain("&#9734;&#9734;&#x2605;  based  on: -"),
            "☆☆★ based on: -"
        );
        assert_eq!(plain("&#bad; &#99999999;"), "&#bad; &#99999999;");
    }
}
