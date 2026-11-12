#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub label: String,
    pub value: String,
}

const LABEL_LIMIT: usize = 40;
const STOPS: [&str; 7] = ["(", "{", "[", ",", ";", ":", ". "];
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
    let mut short: String = label.chars().take(LABEL_LIMIT - 1).collect();
    short.push('…');
    short
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
    let text = ENTITIES.iter().fold(text, |text, (entity, character)| {
        text.replace(entity, character)
    });
    text.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::{label, plain, shown};

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
            "A description that goes on and on witho…"
        );
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
    }
}
