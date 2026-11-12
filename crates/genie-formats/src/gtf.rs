use std::collections::{HashMap, HashSet};

use crate::Reason;
use crate::gff::{self, Record, columns};

pub fn parse(line: &str) -> Result<Option<Record>, Reason> {
    let Some(columns) = columns(line)? else {
        return Ok(None);
    };
    let mut attributes: Vec<(String, String)> = columns
        .attributes
        .split(';')
        .filter_map(|pair| {
            let (key, value) = pair.trim().split_once(char::is_whitespace)?;
            Some((key.to_owned(), value.trim().trim_matches('"').to_owned()))
        })
        .collect();
    let get = |key: &str| {
        attributes
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    };
    let (gene, transcript) = (get("gene_id"), get("transcript_id"));
    let text = gff::text(&attributes);
    let mut parents = Vec::new();
    match columns.kind {
        "gene" => {
            let name = get("gene_name");
            attributes.extend(gene.map(|gene| ("ID".to_owned(), format!("gene:{gene}"))));
            attributes.extend(name.map(|name| ("Name".to_owned(), name)));
        }
        "transcript" => {
            let name = get("transcript_name");
            let id = transcript.map(|transcript| format!("transcript:{transcript}"));
            attributes.extend(id.map(|id| ("ID".to_owned(), id)));
            attributes.extend(name.map(|name| ("Name".to_owned(), name)));
            parents.extend(gene.map(|gene| format!("gene:{gene}")));
        }
        _ => match transcript {
            Some(transcript) => parents.push(format!("transcript:{transcript}")),
            None => parents.extend(gene.map(|gene| format!("gene:{gene}"))),
        },
    }
    Ok(Some(Record {
        contig: columns.contig.to_owned(),
        kind: columns.kind.to_owned(),
        span: columns.span,
        strand: columns.strand,
        parents,
        attributes,
        text,
    }))
}

pub fn complete(records: &mut Vec<Record>) {
    let known: HashSet<String> = records
        .iter()
        .filter_map(|record| record.get("ID").map(str::to_owned))
        .collect();
    let mut added: Vec<Record> = Vec::new();
    let mut slots: HashMap<String, usize> = HashMap::new();
    for record in records.iter() {
        let Some(parent) = record
            .parents
            .iter()
            .find(|parent| parent.starts_with("transcript:") && !known.contains(*parent))
        else {
            continue;
        };
        if let Some(&slot) = slots.get(parent) {
            added[slot].span = added[slot].span.hull(record.span);
            continue;
        }
        let gene = record
            .get("gene_id")
            .map(|gene| format!("gene:{gene}"))
            .filter(|gene| known.contains(gene));
        let mut attributes = vec![("ID".to_owned(), parent.clone())];
        attributes.extend(
            record
                .get("gene_name")
                .map(|name| ("gene_name".to_owned(), name.to_owned())),
        );
        let text: Vec<&str> = record
            .text
            .split('\t')
            .filter(|pair| !pair.starts_with("exon_"))
            .collect();
        slots.insert(parent.clone(), added.len());
        added.push(Record {
            contig: record.contig.clone(),
            kind: "transcript".to_owned(),
            span: record.span,
            strand: record.strand,
            parents: gene.into_iter().collect(),
            attributes,
            text: text.join("\t"),
        });
    }
    records.extend(added);
}

#[cfg(test)]
mod tests {
    use genie_core::{Feature, Region, Strand};

    use super::{complete, parse};
    use crate::gff::{Record, assemble};

    const GENE: &str = "chr1\tsrc\tgene\t1001\t2000\t.\t+\t.\tgene_id \"g1\"; gene_name \"ABC\";";
    const TRANSCRIPT: &str = "chr1\tsrc\ttranscript\t1001\t2000\t.\t+\t.\tgene_id \"g1\"; transcript_id \"t1\"; gene_name \"ABC\"; transcript_name \"ABC-201\";";
    const PARTS: [&str; 5] = [
        "chr1\tsrc\texon\t1001\t1200\t.\t+\t.\tgene_id \"g1\"; transcript_id \"t1\"; gene_name \"ABC\"; exon_number 1;",
        "chr1\tsrc\texon\t1801\t2000\t.\t+\t.\tgene_id \"g1\"; transcript_id \"t1\"; gene_name \"ABC\"; exon_number 2;",
        "chr1\tsrc\tCDS\t1101\t1200\t.\t+\t0\tgene_id \"g1\"; transcript_id \"t1\"; gene_name \"ABC\";",
        "chr1\tsrc\tCDS\t1801\t1900\t.\t+\t2\tgene_id \"g1\"; transcript_id \"t1\"; gene_name \"ABC\";",
        "chr1\tsrc\tfive_prime_utr\t1001\t1100\t.\t+\t.\tgene_id \"g1\"; transcript_id \"t1\";",
    ];

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    fn features(lines: &[&str]) -> Vec<Feature> {
        let mut records: Vec<Record> = lines
            .iter()
            .map(|line| parse(line).unwrap().unwrap())
            .collect();
        complete(&mut records);
        assemble(&records)
    }

    fn transcript() -> Feature {
        Feature::new("ABC", region(1000, 2000), Strand::Forward)
            .with_blocks(vec![region(1000, 1200), region(1800, 2000)])
            .with_coding(Some(region(1100, 1900)))
    }

    #[test]
    fn reads_quoted_and_bare_attributes() {
        let record = parse(PARTS[0]).unwrap().unwrap();
        assert_eq!(record.get("gene_name"), Some("ABC"));
        assert_eq!(record.get("exon_number"), Some("1"));
        assert_eq!(record.parents, ["transcript:t1"]);
        let gene = parse(GENE).unwrap().unwrap();
        assert_eq!(
            (gene.get("ID"), gene.get("Name")),
            (Some("gene:g1"), Some("ABC"))
        );
        assert_eq!(parse("#!genome-build GRCh38"), Ok(None));
    }

    #[test]
    fn assembles_genes_with_transcript_lines() {
        let mut lines = vec![GENE, TRANSCRIPT];
        lines.extend(PARTS);
        let text = "gene_id=g1\ttranscript_id=t1\tgene_name=ABC\ttranscript_name=ABC-201";
        assert_eq!(features(&lines), [transcript().with_extra(text)]);
    }

    #[test]
    fn transcripts_keep_the_attributes_of_the_file() {
        let mut lines = vec![GENE, TRANSCRIPT];
        lines.extend(PARTS);
        assert_eq!(
            features(&lines)[0].extra(),
            "gene_id=g1\ttranscript_id=t1\tgene_name=ABC\ttranscript_name=ABC-201"
        );
        assert_eq!(
            features(&PARTS)[0].extra(),
            "gene_id=g1\ttranscript_id=t1\tgene_name=ABC"
        );
    }

    #[test]
    fn makes_transcripts_that_have_no_line() {
        let text = "gene_id=g1\ttranscript_id=t1\tgene_name=ABC";
        assert_eq!(features(&PARTS), [transcript().with_extra(text)]);
    }
}
