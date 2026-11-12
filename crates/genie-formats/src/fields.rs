#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub label: String,
    pub value: String,
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
    use super::{label, linked, plain, shown};

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
