//! The BLAT server of UCSC: it finds where a sequence aligns in a genome of UCSC, and answers in
//! JSON with the lines of a PSL file.

/// The BLAT page of UCSC.
pub const SERVER: &str = "https://genome.ucsc.edu/cgi-bin/hgBlat";
/// The most bases that genie sends in one search. The address holds the bases, and web servers
/// take addresses of about 8 KB.
pub const MOST: usize = 8_000;
/// The fewest bases that BLAT can align.
pub const FEWEST: usize = 20;

use std::fmt::Write as _;

use genie_core::grouped;

use crate::json::{self, Json};

/// The columns of a PSL line, as the answer names them.
const NAMES: [&str; 21] = [
    "matches",
    "misMatches",
    "repMatches",
    "nCount",
    "qNumInsert",
    "qBaseInsert",
    "tNumInsert",
    "tBaseInsert",
    "strand",
    "qName",
    "qSize",
    "qStart",
    "qEnd",
    "tName",
    "tSize",
    "tStart",
    "tEnd",
    "blockCount",
    "blockSizes",
    "qStarts",
    "tStarts",
];
/// The column of the name of the query.
const QUERY: usize = 9;
const NOT_VALID: &str = "The answer of BLAT is not valid.";

/// The bases of a text in upper case: the letters of DNA without a FASTA header, spaces, line
/// ends and digits. Says why when the text is not a sequence that BLAT can take.
pub fn bases(text: &str) -> Result<String, String> {
    let mut found = String::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('>') {
            continue;
        }
        for letter in line.chars() {
            if letter.is_ascii_whitespace() || letter.is_ascii_digit() {
                continue;
            }
            let upper = letter.to_ascii_uppercase();
            if !"ACGTN".contains(upper) {
                return Err(format!(
                    "The sequence has letters that are not bases, such as {letter}."
                ));
            }
            found.push(upper);
        }
    }
    if found.len() < FEWEST {
        return Err(format!("BLAT needs at least {FEWEST} bases."));
    }
    if found.len() > MOST {
        return Err(format!(
            "genie sends at most {} bases to BLAT.",
            grouped(MOST as u64)
        ));
    }
    Ok(found)
}

/// The address that asks a BLAT server for the alignments of bases in a genome of UCSC. UCSC
/// takes searches from programs only with the API key of a person.
pub fn address(server: &str, genome: &str, bases: &str, key: Option<&str>) -> String {
    let mut address = format!("{}&output=json", page(server, genome, bases));
    if let Some(key) = key.map(str::trim).filter(|key| !key.is_empty()) {
        address.push_str("&apiKey=");
        for byte in key.bytes() {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                address.push(char::from(byte));
            } else {
                let _ = write!(address, "%{byte:02X}");
            }
        }
    }
    address
}

/// The address of the web page of the same search, for a web browser.
pub fn page(server: &str, genome: &str, bases: &str) -> String {
    format!("{server}?userSeq={bases}&type=DNA&db={genome}")
}

/// True when an answer is a web page, not JSON: UCSC answers a program without a valid key with
/// a page that checks for people.
pub fn is_page(answer: &str) -> bool {
    answer.trim_start().starts_with('<')
}

/// The alignments of an answer of the server as the lines of a PSL file, with the query name of
/// each line. Says why when the answer has an error or is not valid.
pub fn psl(answer: &str, query: &str) -> Result<String, String> {
    let not_valid = || NOT_VALID.to_owned();
    let found = json::parse(answer).map_err(|_| not_valid())?;
    if let Some(error) = found.get("error").and_then(Json::text) {
        return Err(format!("BLAT says: {error}"));
    }
    let rows = found
        .get("blat")
        .and_then(Json::list)
        .ok_or_else(not_valid)?;
    let places: Vec<usize> = match found.get("fields").and_then(Json::list) {
        Some(fields) => NAMES
            .iter()
            .map(|name| {
                fields
                    .iter()
                    .position(|field| field.text().as_deref() == Some(*name))
            })
            .collect::<Option<Vec<usize>>>()
            .ok_or_else(not_valid)?,
        None => (0..NAMES.len()).collect(),
    };
    let name: String = query
        .chars()
        .map(|letter| if letter.is_whitespace() { '_' } else { letter })
        .collect();
    let mut lines = Vec::new();
    for row in rows {
        let values = row.list().ok_or_else(not_valid)?;
        let columns = places
            .iter()
            .enumerate()
            .map(|(column, &place)| {
                if column == QUERY {
                    return Some(name.clone());
                }
                values.get(place).and_then(Json::text)
            })
            .collect::<Option<Vec<String>>>()
            .ok_or_else(not_valid)?;
        let line = columns.join("\t");
        crate::psl::parse(&line)
            .ok()
            .flatten()
            .ok_or_else(not_valid)?;
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{FEWEST, MOST, address, bases, is_page, page, psl};
    use crate::psl::parse;

    #[test]
    fn bases_come_from_fasta_or_plain_text() {
        assert_eq!(
            bases(">read7 some words\nacgtacgtac gtacgtacgt\r\nNNACGT\n").as_deref(),
            Ok("ACGTACGTACGTACGTACGTNNACGT")
        );
        assert_eq!(
            bases("10 acgtacgtac 20 gtacgtacgt").as_deref(),
            Ok("ACGTACGTACGTACGTACGT")
        );
        let short = bases("ACGT").unwrap_err();
        assert_eq!(short, format!("BLAT needs at least {FEWEST} bases."));
        let long = bases(&"A".repeat(MOST + 1)).unwrap_err();
        assert_eq!(long, "genie sends at most 8,000 bases to BLAT.");
        let protein = bases("MKTAYIAKQRQISFVKSHFSRQ").unwrap_err();
        assert_eq!(
            protein,
            "The sequence has letters that are not bases, such as M."
        );
    }

    #[test]
    fn addresses_ask_for_json_with_the_key() {
        let server = "https://genome.ucsc.edu/cgi-bin/hgBlat";
        assert_eq!(
            address(server, "hg38", "ACGT", None),
            "https://genome.ucsc.edu/cgi-bin/hgBlat?userSeq=ACGT&type=DNA&db=hg38&output=json"
        );
        assert_eq!(
            address(server, "hg38", "ACGT", Some(" a1/b+\n")),
            "https://genome.ucsc.edu/cgi-bin/hgBlat?userSeq=ACGT&type=DNA&db=hg38&output=json&apiKey=a1%2Fb%2B"
        );
        assert_eq!(
            address(server, "hg38", "ACGT", Some("  ")),
            address(server, "hg38", "ACGT", None)
        );
        assert_eq!(
            page(server, "hg38", "ACGT"),
            "https://genome.ucsc.edu/cgi-bin/hgBlat?userSeq=ACGT&type=DNA&db=hg38"
        );
    }

    /// UCSC answers a program without a valid key with a web page that checks for people.
    #[test]
    fn web_pages_are_not_answers() {
        assert!(is_page("\n<html><head><script>"));
        assert!(!is_page(ANSWER));
    }

    const ANSWER: &str = r#"{"track": "blat", "genome": "hg38",
        "fields": ["matches", "misMatches", "repMatches", "nCount", "qNumInsert", "qBaseInsert",
            "tNumInsert", "tBaseInsert", "strand", "qName", "qSize", "qStart", "qEnd", "tName",
            "tSize", "tStart", "tEnd", "blockCount", "blockSizes", "qStarts", "tStarts"],
        "blat": [
            [48, 2, 0, 0, 0, 0, 1, 300, "-", "YourSeq", 50, 0, 50, "chr17", 83257441,
             43044294, 43044644, 2, "20,30,", "0,20,", "43044294,43044614,"],
            [24, 0, 0, 0, 0, 0, 0, 0, "+", "YourSeq", 50, 10, 34, "chr2", 242193529,
             1000, 1024, 1, "24,", "10,", "1000,"]
        ]}"#;

    #[test]
    fn answers_become_psl_lines_with_the_query_name() {
        let text = psl(ANSWER, "read7").unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            "48\t2\t0\t0\t0\t0\t1\t300\t-\tread7\t50\t0\t50\tchr17\t83257441\t43044294\t43044644\t2\t20,30,\t0,20,\t43044294,43044614,"
        );
        let (contig, feature) = parse(lines[1]).unwrap().unwrap();
        assert_eq!((contig, feature.name()), ("chr2", "read7"));
    }

    #[test]
    fn answers_in_another_order_follow_their_field_names() {
        let swapped = ANSWER.replace(r#""matches", "misMatches""#, r#""misMatches", "matches""#);
        let text = psl(&swapped, "q").unwrap();
        assert!(text.starts_with("2\t48\t"), "{text}");
    }

    #[test]
    fn empty_wrong_and_failed_answers() {
        let none = r#"{"track": "blat", "genome": "hg38", "blat": []}"#;
        assert_eq!(psl(none, "q").as_deref(), Ok(""));
        assert_eq!(
            psl(r#"{"error": "Sequence too long"}"#, "q"),
            Err("BLAT says: Sequence too long".to_owned())
        );
        assert_eq!(
            psl("<html>busy</html>", "q"),
            Err("The answer of BLAT is not valid.".to_owned())
        );
        let short = r#"{"blat": [[1, 2, 3]]}"#;
        assert_eq!(
            psl(short, "q"),
            Err("The answer of BLAT is not valid.".to_owned())
        );
    }
}
