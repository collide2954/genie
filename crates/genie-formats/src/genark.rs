//! UCSC `GenArk`: the assembly hubs that UCSC makes for the assemblies of NCBI. Each hub has
//! the `GenBank` (GCA) or `RefSeq` (GCF) accession of its assembly in its address.

/// The list of all `GenArk` assemblies, a tab-separated text file.
pub const LIST: &str = "https://hgdownload.soe.ucsc.edu/hubs/UCSC_GI.assemblyHubList.txt";
/// The folder of the `GenArk` hubs.
const HUBS: &str = "https://hgdownload.soe.ucsc.edu/hubs";

/// An assembly of the `GenArk` list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembly {
    /// The accession with its version, as `GCF_000001405.40`.
    pub accession: String,
    /// The name of the assembly, as GRCh38.p14.
    pub name: String,
    pub species: String,
    /// The common name, as human, or empty.
    pub common: String,
}

/// The names of the columns of the list that genie uses, in the order of a list without a header.
const COLUMNS: [&str; 4] = ["accession", "assembly", "scientific name", "common name"];

/// The accession of an NCBI assembly in a text, in upper case: GCA or GCF, an underscore, nine
/// digits, a dot and the version, as `GCF_000001405.40`.
pub fn accession(text: &str) -> Option<String> {
    let text = text.trim().to_ascii_uppercase();
    let rest = text
        .strip_prefix("GCA_")
        .or_else(|| text.strip_prefix("GCF_"))?;
    let (digits, version) = rest.split_once('.')?;
    let numbers = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    (digits.len() == 9 && numbers(digits) && numbers(version)).then_some(text)
}

/// The address of the `hub.txt` file of the `GenArk` hub of an accession.
pub fn hub(accession: &str) -> Option<String> {
    let accession = self::accession(accession)?;
    let (kind, digits) = (&accession[..3], &accession[4..13]);
    let (first, second, third) = (&digits[..3], &digits[3..6], &digits[6..]);
    Some(format!(
        "{HUBS}/{kind}/{first}/{second}/{third}/{accession}/hub.txt"
    ))
}

/// The assemblies of the `GenArk` list. A header line that starts with `#` names the columns, and
/// genie uses the accession, the assembly, the scientific name and the common name. Without a
/// header, the columns come in this order. Lines without an accession do not count.
pub fn list(text: &str) -> Vec<Assembly> {
    let mut places: [Option<usize>; 4] = [Some(0), Some(1), Some(2), Some(3)];
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(header) = line.strip_prefix('#') {
            let names: Vec<&str> = header.split('\t').map(str::trim).collect();
            if names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(COLUMNS[0]))
            {
                places = COLUMNS.map(|wanted| {
                    names
                        .iter()
                        .position(|name| name.eq_ignore_ascii_case(wanted))
                });
            }
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let value = |place: Option<usize>| {
            place
                .and_then(|place| fields.get(place))
                .map_or_else(String::new, |value| value.trim().to_owned())
        };
        let Some(accession) = accession(&value(places[0])) else {
            continue;
        };
        found.push(Assembly {
            accession,
            name: value(places[1]),
            species: value(places[2]),
            common: value(places[3]),
        });
    }
    found
}

/// The assemblies whose accession, name, species or common name hold all words of a query, at
/// most `most` of them. The accessions that start with the query come first, then the species
/// and the common names that start with it, then the others in the order of the list.
pub fn search<'a>(assemblies: &'a [Assembly], query: &str, most: usize) -> Vec<&'a Assembly> {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(u8, &Assembly)> = assemblies
        .iter()
        .filter_map(|assembly| {
            let parts = [
                assembly.accession.to_lowercase(),
                assembly.name.to_lowercase(),
                assembly.species.to_lowercase(),
                assembly.common.to_lowercase(),
            ];
            let all = parts.join(" ");
            if !words.iter().all(|word| all.contains(word)) {
                return None;
            }
            let rank = [0, 2, 3]
                .into_iter()
                .zip([0, 1, 2])
                .find(|(part, _)| parts[*part].starts_with(&query))
                .map_or(3, |(_, rank)| rank);
            Some((rank, assembly))
        })
        .collect();
    found.sort_by_key(|(rank, _)| *rank);
    found
        .into_iter()
        .take(most)
        .map(|(_, assembly)| assembly)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Assembly, accession, hub, list, search};

    const TEXT: &str = "# UCSC GenArk assembly hubs\n\
        # accession\tassembly\tscientific name\tcommon name\ttaxonId\n\
        GCA_009914755.4\tT2T-CHM13v2.0\tHomo sapiens\thuman (2022 T2T)\t9606\n\
        GCF_000001405.40\tGRCh38.p14\tHomo sapiens\thuman\t9606\n\
        GCF_016699485.2\tbGalGal1.mat.broiler.GRCg7b\tGallus gallus\tchicken\t9031\n\
        GCF_000002315.6\tGRCg6a\tGallus gallus\tchicken (2018)\t9031\n\
        GCA_011100615.1\tMus caroli assembly\tMus caroli\tRyukyu mouse\t10089\n";

    #[test]
    fn accessions_have_a_kind_nine_digits_and_a_version() {
        assert_eq!(
            accession(" gcf_000001405.40 ").as_deref(),
            Some("GCF_000001405.40")
        );
        assert_eq!(
            accession("GCA_009914755.4").as_deref(),
            Some("GCA_009914755.4")
        );
        for wrong in [
            "GCF_000001405",
            "GCX_000001405.40",
            "GCF_00001405.40",
            "GCF_000001405.x",
        ] {
            assert_eq!(accession(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn hubs_are_in_folders_of_three_digits() {
        assert_eq!(
            hub("GCF_000001405.40").as_deref(),
            Some("https://hgdownload.soe.ucsc.edu/hubs/GCF/000/001/405/GCF_000001405.40/hub.txt")
        );
        assert_eq!(
            hub("GCA_009914755.4").as_deref(),
            Some("https://hgdownload.soe.ucsc.edu/hubs/GCA/009/914/755/GCA_009914755.4/hub.txt")
        );
        assert_eq!(hub("chr1"), None);
    }

    #[test]
    fn lists_name_their_columns_in_a_header() {
        let found = list(TEXT);
        assert_eq!(found.len(), 5);
        assert_eq!(
            found[1],
            Assembly {
                accession: "GCF_000001405.40".to_owned(),
                name: "GRCh38.p14".to_owned(),
                species: "Homo sapiens".to_owned(),
                common: "human".to_owned(),
            }
        );
        let other = "#common name\tscientific name\taccession\nfruit fly\tDrosophila melanogaster\tGCF_000001215.4\n";
        let fly = list(other);
        assert_eq!(fly[0].accession, "GCF_000001215.4");
        assert_eq!(fly[0].species, "Drosophila melanogaster");
        assert_eq!(fly[0].name, "");
        let plain = list("GCF_000001405.40\tGRCh38.p14\tHomo sapiens\thuman\nbad line\n");
        assert_eq!(plain.len(), 1);
        assert_eq!(plain[0].common, "human");
    }

    fn accessions(found: &[&Assembly]) -> Vec<String> {
        found.iter().map(|found| found.accession.clone()).collect()
    }

    #[test]
    fn searches_find_species_common_names_and_accessions() {
        let all = list(TEXT);
        assert_eq!(
            accessions(&search(&all, "chicken", 10)),
            ["GCF_016699485.2", "GCF_000002315.6"]
        );
        assert_eq!(
            accessions(&search(&all, "gallus GRCg6a", 10)),
            ["GCF_000002315.6"]
        );
        assert_eq!(
            accessions(&search(&all, "gcf_000001405", 10)),
            ["GCF_000001405.40"]
        );
        assert_eq!(accessions(&search(&all, "mouse", 10)), ["GCA_011100615.1"]);
        let human = search(&all, "Homo", 1);
        assert_eq!(accessions(&human), ["GCA_009914755.4"]);
        assert_eq!(search(&all, "  ", 10).len(), 0);
    }
}
