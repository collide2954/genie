//! Protein domains of the TSV files of InterProScan and of the output of `pfam_scan.pl`: each
//! domain is a run of amino acids of a protein, with the database and the signature that found
//! it.

/// A domain of a protein: the protein, its first and its last amino acid from 1, the database,
/// the accession and the name of the signature, a description, and the InterPro entry when the
/// file gives one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Domain {
    pub protein: String,
    pub first: u64,
    pub last: u64,
    pub database: String,
    pub accession: String,
    pub name: String,
    pub description: String,
    pub interpro: Option<(String, String)>,
}

impl Domain {
    /// The name of the domain with its accession, as "Pkinase (PF00069)".
    pub fn title(&self) -> String {
        if self.name == self.accession || self.accession.is_empty() {
            self.name.clone()
        } else {
            format!("{} ({})", self.name, self.accession)
        }
    }
}

/// The kinds of files of domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The TSV output of InterProScan.
    InterProScan,
    /// The output of `pfam_scan.pl`.
    PfamScan,
}

fn number(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

/// The domain of a line of InterProScan: the protein, the MD5 of its sequence, its length, the
/// database, the accession and the description of the signature, the first and the last amino
/// acid, the score, the status, the date, and the accession and the description of the
/// InterPro entry, or `-`.
fn interpro_line(line: &str) -> Option<Domain> {
    let columns: Vec<&str> = line.split('\t').map(str::trim).collect();
    if columns.len() < 11 || columns[0].is_empty() || columns[3].is_empty() {
        return None;
    }
    number(columns[2])?;
    let (first, last) = (number(columns[6])?, number(columns[7])?);
    if first == 0 || last < first {
        return None;
    }
    let known = |text: &str| !text.is_empty() && text != "-";
    let accession = columns[4].to_owned();
    let description = if known(columns[5]) {
        columns[5].to_owned()
    } else {
        String::new()
    };
    let interpro = match (columns.get(11), columns.get(12)) {
        (Some(entry), description) if known(entry) => Some((
            (*entry).to_owned(),
            description
                .filter(|text| known(text))
                .map_or_else(String::new, |text| (*text).to_owned()),
        )),
        _ => None,
    };
    Some(Domain {
        protein: columns[0].to_owned(),
        first,
        last,
        database: columns[3].to_owned(),
        name: if description.is_empty() {
            accession.clone()
        } else {
            description.clone()
        },
        accession,
        description,
        interpro,
    })
}

/// The domain of a line of `pfam_scan.pl`: the protein, the start and the end of the
/// alignment and of the envelope, the accession of the model with its version, its name, its
/// type, and more columns. The domain is the envelope.
fn pfam_line(line: &str) -> Option<Domain> {
    let columns: Vec<&str> = line.split_whitespace().collect();
    if columns.len() < 8 || !columns[5].starts_with("PF") {
        return None;
    }
    for column in &columns[1..5] {
        number(column)?;
    }
    let (first, last) = (number(columns[3])?, number(columns[4])?);
    if first == 0 || last < first {
        return None;
    }
    let accession = columns[5]
        .split('.')
        .next()
        .unwrap_or(columns[5])
        .to_owned();
    Some(Domain {
        protein: columns[0].to_owned(),
        first,
        last,
        database: "Pfam".to_owned(),
        accession,
        name: columns[6].to_owned(),
        description: columns[7].to_owned(),
        interpro: None,
    })
}

impl Layout {
    /// The layout of a file of domains from its first lines, if it is one: lines of InterProScan
    /// with 11 tab-separated columns or more, or lines of `pfam_scan.pl`, after comments that
    /// start with `#`.
    pub fn sniff(lines: &[String]) -> Option<Self> {
        let mut data = lines
            .iter()
            .map(|line| line.trim_end_matches(['\n', '\r']))
            .filter(|line| !line.trim().is_empty() && !line.starts_with('#'));
        let first = data.next()?;
        if interpro_line(first).is_some() {
            return Some(Self::InterProScan);
        }
        pfam_line(first).map(|_| Self::PfamScan)
    }
}

/// The domains of the text of a file of domains. Lines that are not domains do not count.
pub fn parse(text: &str, layout: Layout) -> Vec<Domain> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| match layout {
            Layout::InterProScan => interpro_line(line),
            Layout::PfamScan => pfam_line(line),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Domain, Layout, parse};

    const INTERPRO: &str = "P04637\t2ccff2d2\t393\tPfam\tPF00870\tP53 DNA-binding domain\t95\t289\t1.2E-90\tT\t15-03-2013\tIPR011615\tp53, DNA-binding domain\tGO:0003677\t\n\
P04637\t2ccff2d2\t393\tMobiDBLite\tmobidb-lite\tconsensus disorder prediction\t1\t40\t-\tT\t15-03-2013\t-\t-\t\t\n\
P04637\t2ccff2d2\t393\tCoils\tCoil\t-\t330\t357\t-\tT\t15-03-2013\t-\t-\t\t\n";

    const PFAM: &str = "# pfam_scan.pl,  run at Fri Oct 26 12:00:00 2026\n\
#\n\
# <seq id> <alignment start> <alignment end> <envelope start> <envelope end> <hmm acc> <hmm name> <type> <hmm start> <hmm end> <hmm length> <bit score> <E-value> <significance> <clan>\n\
\n\
ENSP00000269305.4     98    288     95    289 PF00870.21  P53               Domain     4   194   196    310.2   1.1e-92   1 CL0073\n\
ENSP00000269305.4    319    356    318    358 PF07710.16  P53_tetramer      Domain     2    39    40     68.0   4.2e-19   1 No_clan\n";

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    #[test]
    fn interproscan_files_give_the_signatures_of_each_protein() {
        assert_eq!(Layout::sniff(&lines(INTERPRO)), Some(Layout::InterProScan));
        let found = parse(INTERPRO, Layout::InterProScan);
        assert_eq!(found.len(), 3);
        assert_eq!(
            found[0],
            Domain {
                protein: "P04637".to_owned(),
                first: 95,
                last: 289,
                database: "Pfam".to_owned(),
                accession: "PF00870".to_owned(),
                name: "P53 DNA-binding domain".to_owned(),
                description: "P53 DNA-binding domain".to_owned(),
                interpro: Some(("IPR011615".to_owned(), "p53, DNA-binding domain".to_owned())),
            }
        );
        assert_eq!(found[0].title(), "P53 DNA-binding domain (PF00870)");
        assert_eq!(found[1].interpro, None);
        assert_eq!(found[2].title(), "Coil");
    }

    #[test]
    fn pfam_scan_files_give_the_envelopes_of_their_domains() {
        assert_eq!(Layout::sniff(&lines(PFAM)), Some(Layout::PfamScan));
        let found = parse(PFAM, Layout::PfamScan);
        let shown: Vec<(&str, u64, u64, String)> = found
            .iter()
            .map(|domain| {
                (
                    domain.protein.as_str(),
                    domain.first,
                    domain.last,
                    domain.title(),
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("ENSP00000269305.4", 95, 289, "P53 (PF00870)".to_owned()),
                (
                    "ENSP00000269305.4",
                    318,
                    358,
                    "P53_tetramer (PF07710)".to_owned()
                ),
            ]
        );
        assert_eq!(
            (found[0].database.as_str(), found[0].description.as_str()),
            ("Pfam", "Domain")
        );
    }

    #[test]
    fn other_tables_are_not_domains() {
        for text in [
            "sample\tpopulation\tsex\nNA12878\tCEU\tF\n",
            "chr1\t100\t200\tgene\n",
            "",
            "P04637\tx\tlong\tPfam\tPF1\td\t1\t2\t-\tT\t1\n",
        ] {
            assert_eq!(Layout::sniff(&lines(text)), None, "{text}");
        }
    }
}
