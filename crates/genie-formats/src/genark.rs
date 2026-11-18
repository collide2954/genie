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
    /// True for an outdated assembly that a newer assembly replaces. UCSC puts it in its legacy
    /// set, and its clade ends with `(L)`.
    pub legacy: bool,
    /// The accession, the name, the species and the common name in lower case, for searches.
    words: String,
}

/// The names of the columns of the list that genie uses.
const COLUMNS: [&str; 5] = [
    "accession",
    "assembly",
    "scientific name",
    "common name",
    "genark clade",
];

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
/// genie uses the accession, the assembly, the scientific name, the common name and the clade.
/// Without a header, these are the columns 1 to 4 and 6. Lines without an accession do not count.
pub fn list(text: &str) -> Vec<Assembly> {
    let mut places: [Option<usize>; 5] = [Some(0), Some(1), Some(2), Some(3), Some(5)];
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
        let (name, species, common) = (value(places[1]), value(places[2]), value(places[3]));
        found.push(Assembly {
            words: format!("{accession} {name} {species} {common}").to_lowercase(),
            legacy: value(places[4]).ends_with("(L)"),
            accession,
            name,
            species,
            common,
        });
    }
    found
}

/// The assemblies whose accession, name, species or common name hold all words of a query, at
/// most `most` of them. The accessions that start with the query come first, then the species
/// and the common names that start with it, then the others in the order of the list. In each
/// of these groups, the outdated assemblies come last.
pub fn search<'a>(assemblies: &'a [Assembly], query: &str, most: usize) -> Vec<&'a Assembly> {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let starts = |text: &str| {
        text.as_bytes()
            .get(..query.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(query.as_bytes()))
    };
    let mut found: Vec<(u8, bool, &Assembly)> = assemblies
        .iter()
        .filter(|assembly| words.iter().all(|word| assembly.words.contains(word)))
        .map(|assembly| {
            let rank = [&assembly.accession, &assembly.species, &assembly.common]
                .into_iter()
                .zip([0, 1, 2])
                .find(|(part, _)| starts(part))
                .map_or(3, |(_, rank)| rank);
            (rank, assembly.legacy, assembly)
        })
        .collect();
    found.sort_by_key(|(rank, legacy, _)| (*rank, *legacy));
    found
        .into_iter()
        .take(most)
        .map(|(_, _, assembly)| assembly)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Assembly, accession, hub, list, search};

    const TEXT: &str = "# UCSC GenArk assembly hubs\n\
        # accession\tassembly\tscientific name\tcommon name\ttaxonId\tGenArk clade\n\
        GCA_000002315.3\tGallus_gallus-4.0\tGallus gallus\tchicken (2011 genbank)\t9031\tbirds(L)\n\
        GCA_009914755.4\tT2T-CHM13v2.0\tHomo sapiens\thuman (2022 T2T)\t9606\tprimates\n\
        GCF_000001405.40\tGRCh38.p14\tHomo sapiens\thuman\t9606\tprimates\n\
        GCF_016699485.2\tbGalGal1.mat.broiler.GRCg7b\tGallus gallus\tchicken\t9031\tbirds\n\
        GCF_000002315.6\tGRCg6a\tGallus gallus\tchicken (2018)\t9031\tbirds\n\
        GCA_011100615.1\tMus caroli assembly\tMus caroli\tRyukyu mouse\t10089\tmammals\n";

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

    fn fields(assembly: &Assembly) -> [&str; 4] {
        let Assembly {
            accession,
            name,
            species,
            common,
            ..
        } = assembly;
        [accession, name, species, common].map(String::as_str)
    }

    #[test]
    fn lists_name_their_columns_in_a_header() {
        let found = list(TEXT);
        assert_eq!(found.len(), 6);
        assert_eq!(
            fields(&found[2]),
            ["GCF_000001405.40", "GRCh38.p14", "Homo sapiens", "human"]
        );
        let legacy: Vec<bool> = found.iter().map(|assembly| assembly.legacy).collect();
        assert_eq!(legacy, [true, false, false, false, false, false]);
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
            ["GCF_016699485.2", "GCF_000002315.6", "GCA_000002315.3"]
        );
        assert_eq!(
            accessions(&search(&all, "gca_000002315", 10)),
            ["GCA_000002315.3"]
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
