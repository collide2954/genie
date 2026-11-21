//! Sample tables: a row for each sample, with fields such as population, sex and phenotype.
//! genie reads tab or comma separated files with a header line (for example the `.panel`
//! files of the 1000 Genomes Project and IGV sample information files) and PLINK `.fam` and
//! `.ped` files.

use std::io::BufRead;
use std::path::Path;

use genie_core::samples::Table;

use crate::text::lines;
use crate::{Error, Reason};

/// Names of the column with the sample names, compared without case. When the header has
/// none of them, the first column has the sample names.
const SAMPLES: [&str; 8] = [
    "sample",
    "sample_id",
    "sampleid",
    "sample_name",
    "samplename",
    "id",
    "iid",
    "name",
];

/// The fields of a PLINK file after the family and the sample name.
const PLINK: [&str; 5] = ["Family", "Father", "Mother", "Sex", "Phenotype"];
/// The values of a case and control phenotype in PLINK files.
const CASES: [&str; 4] = ["1", "2", "0", "-9"];

/// True for PLINK `.fam` and `.ped` files, also when compressed.
pub fn is_plink(name: &str) -> bool {
    let path = Path::new(name);
    let compressed = path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("gz") || extension.eq_ignore_ascii_case("bgz")
    });
    let inner = if compressed {
        path.file_stem().map(Path::new)
    } else {
        Some(path)
    };
    inner.and_then(Path::extension).is_some_and(|extension| {
        extension.eq_ignore_ascii_case("fam") || extension.eq_ignore_ascii_case("ped")
    })
}

/// Reads a sample table. The name of the file tells a PLINK file from a table with a header.
pub fn read(reader: impl BufRead, name: &str) -> Result<Table, Error> {
    if is_plink(name) {
        plink(reader)
    } else {
        delimited(reader)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// A file without sections, or the `#sampleTable` section of an IGV file.
    Table,
    /// The `#sampleMapping` and `#colors` sections of an IGV file.
    Other,
}

/// How the fields of a line are apart: tabs, commas or spaces.
fn delimiter(line: &str) -> Option<char> {
    if line.contains('\t') {
        Some('\t')
    } else if line.contains(',') {
        Some(',')
    } else if line.trim().contains(char::is_whitespace) {
        Some(' ')
    } else {
        None
    }
}

/// A field without the quotes around it.
fn unquoted(field: &str) -> String {
    let field = field.trim();
    match field
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
    {
        Some(inner) => inner.replace("\"\"", "\""),
        None => field.to_owned(),
    }
}

/// The fields of a comma separated line. A field in double quotes can hold commas, and two
/// double quotes in it are one double quote.
fn commas(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut letters = line.chars().peekable();
    while let Some(letter) = letters.next() {
        match letter {
            '"' if quoted && letters.peek() == Some(&'"') => {
                field.push('"');
                letters.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut field).trim().to_owned()),
            letter => field.push(letter),
        }
    }
    fields.push(field.trim().to_owned());
    fields
}

fn split(line: &str, delimiter: char) -> Vec<String> {
    match delimiter {
        ',' => commas(line),
        ' ' => line.split_whitespace().map(unquoted).collect(),
        _ => line.split(delimiter).map(unquoted).collect(),
    }
}

fn is_sample_column(name: &str) -> bool {
    SAMPLES.iter().any(|known| name.eq_ignore_ascii_case(known))
}

/// The lines of a table with a header line: the names of the columns, the rows, and the names
/// to show from the first line of a cBioPortal file.
struct Parsed {
    names: Vec<String>,
    rows: Vec<Vec<String>>,
    shown: Option<Vec<String>>,
}

fn delimited(reader: impl BufRead) -> Result<Table, Error> {
    let parsed = parse(reader, Reason::SampleTable)?;
    let sample = parsed
        .names
        .iter()
        .position(|name| is_sample_column(name))
        .unwrap_or(0);
    Ok(table(parsed, sample))
}

/// The most rows that tell if a column has barcodes.
const BARCODE_ROWS: usize = 100;

/// Reads a table of the cells of a single-cell data set, as the meta.data of Seurat or the obs
/// of Scanpy written as CSV or TSV: a header line and a row for each cell. The barcodes of the
/// cells are in the first column whose values are mostly barcodes, else in the first column.
pub fn read_cells(reader: impl BufRead) -> Result<Table, Error> {
    let parsed = parse(reader, Reason::CellTable)?;
    let barcodes = |column: usize| {
        let values: Vec<&String> = parsed
            .rows
            .iter()
            .take(BARCODE_ROWS)
            .filter_map(|row| row.get(column))
            .collect();
        let found = values
            .iter()
            .filter(|value| genie_core::cells::core(value).is_some())
            .count();
        found * 2 > values.len()
    };
    let column = (0..parsed.names.len()).find(|&column| barcodes(column));
    Ok(table(parsed, column.unwrap_or(0)))
}

/// The table of parsed lines with the names of the rows in a column.
fn table(parsed: Parsed, names_column: usize) -> Table {
    let Parsed { names, rows, shown } = parsed;
    let others = |fields: Vec<String>| -> Vec<String> {
        fields
            .into_iter()
            .enumerate()
            .filter(|(place, _)| *place != names_column)
            .map(|(_, field)| field)
            .collect()
    };
    let rows = rows
        .into_iter()
        .map(|mut fields| {
            fields.resize(names.len(), String::new());
            let name = fields[names_column].clone();
            (name, others(fields))
        })
        .collect();
    let columns = others(match shown {
        Some(mut shown) => {
            shown.truncate(names.len());
            shown
        }
        None => names,
    });
    Table::new(columns, rows)
}

/// The lines of a tab or comma separated table, or an error with the reason `invalid` when the
/// file has no header line or no rows.
fn parse(reader: impl BufRead, invalid: Reason) -> Result<Parsed, Error> {
    let mut section = Section::Table;
    let mut header: Option<(Vec<String>, char)> = None;
    // Lines that start with # and can be the header, as in "#sample<tab>population". Two or
    // more of them before the first other line are the lines of a cBioPortal file, and the
    // first of them gives the names to show.
    let mut candidates: Vec<(Vec<String>, char)> = Vec::new();
    let mut shown: Option<Vec<String>> = None;
    let mut rows: Vec<Vec<String>> = Vec::new();
    lines(reader, |_, line| {
        if let Some(comment) = line.strip_prefix('#') {
            let marker = comment.trim();
            if marker.eq_ignore_ascii_case("sampleTable") {
                section = Section::Table;
            } else if marker.eq_ignore_ascii_case("sampleMapping")
                || marker.eq_ignore_ascii_case("colors")
            {
                section = Section::Other;
            } else if section == Section::Table && header.is_none() {
                let found = delimiter(marker).filter(|&delimiter| {
                    delimiter == '\t'
                        || split(marker, delimiter)
                            .first()
                            .is_some_and(|name| is_sample_column(name))
                });
                if let Some(delimiter) = found {
                    candidates.push((split(marker, delimiter), delimiter));
                }
            }
            return Ok(());
        }
        if section == Section::Other {
            return Ok(());
        }
        match &header {
            None if candidates.len() == 1 => {
                let (names, delimiter) = candidates.remove(0);
                rows.push(split(line, delimiter));
                header = Some((names, delimiter));
            }
            None => {
                let delimiter = delimiter(line).unwrap_or('\t');
                let names = split(line, delimiter);
                shown = candidates
                    .first()
                    .map(|(first, _)| first.clone())
                    .filter(|first| first.len() == names.len());
                header = Some((names, delimiter));
            }
            Some((_, delimiter)) => rows.push(split(line, *delimiter)),
        }
        Ok(())
    })?;
    if header.is_none() && !candidates.is_empty() {
        header = Some(candidates.remove(0));
    }
    let Some((mut names, _)) = header else {
        return Err(Error::Invalid(invalid));
    };
    while names.last().is_some_and(String::is_empty) {
        names.pop();
    }
    if names.len() < 2 || rows.is_empty() {
        return Err(Error::Invalid(invalid));
    }
    Ok(Parsed { names, rows, shown })
}

fn plink(reader: impl BufRead) -> Result<Table, Error> {
    let mut found: Vec<Vec<String>> = Vec::new();
    lines(reader, |number, line| {
        if line.starts_with('#') {
            return Ok(());
        }
        let fields: Vec<String> = line
            .split_whitespace()
            .take(PLINK.len() + 1)
            .map(str::to_owned)
            .collect();
        if fields.len() <= PLINK.len() {
            return Err(Error::Format {
                line: number,
                reason: Reason::MissingField,
            });
        }
        found.push(fields);
        Ok(())
    })?;
    if found.is_empty() {
        return Err(Error::Invalid(Reason::SampleTable));
    }
    // A phenotype of only 1, 2, 0 and -9 is a case and control phenotype; others are numbers.
    let cases = found
        .iter()
        .all(|fields| CASES.contains(&fields[5].as_str()));
    let parent = |id: &str| {
        if id == "0" {
            String::new()
        } else {
            id.to_owned()
        }
    };
    let rows = found
        .into_iter()
        .map(|fields| {
            let sex = match fields[4].as_str() {
                "1" => "male",
                "2" => "female",
                _ => "",
            };
            let phenotype = match fields[5].as_str() {
                "-9" => "",
                "0" if cases => "",
                "1" if cases => "unaffected",
                "2" if cases => "affected",
                other => other,
            };
            let values = vec![
                fields[0].clone(),
                parent(&fields[2]),
                parent(&fields[3]),
                sex.to_owned(),
                phenotype.to_owned(),
            ];
            (fields[1].clone(), values)
        })
        .collect();
    Ok(Table::new(PLINK.map(str::to_owned).to_vec(), rows))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use genie_core::samples::Table;

    use super::{is_plink, read, read_cells};
    use crate::{Error, Reason};

    fn opened(text: &str, name: &str) -> Table {
        read(Cursor::new(text), name).unwrap()
    }

    #[test]
    fn cell_tables_take_the_barcodes_from_the_first_column_with_barcodes() {
        let seurat = "\"\",\"orig.ident\",\"nCount_RNA\",\"seurat_clusters\",\"cell_type\"\n\
            \"AAACCCAAGAAACACT-1\",\"pbmc\",4520,\"0\",\"CD4 T\"\n\
            \"AAACCCAAGAAACCAT-1\",\"pbmc\",3011,\"2\",\"B\"\n";
        let table = read_cells(Cursor::new(seurat)).unwrap();
        assert_eq!(
            table.columns,
            ["orig.ident", "nCount_RNA", "seurat_clusters", "cell_type"]
        );
        assert_eq!(table.value("AAACCCAAGAAACCAT-1", 3), Some("B"));
        let scanpy = ",sample,n_genes,leiden\nAAACCCAAGAAACACT-1-0,s1,1200,3\n";
        let table = read_cells(Cursor::new(scanpy)).unwrap();
        assert_eq!(
            table.columns,
            ["sample", "n_genes", "leiden"],
            "a sample column does not name the cells"
        );
        assert!(table.has("AAACCCAAGAAACACT-1-0"));
        let later = "cell_type\tbarcode\nT\tAAACCCAAGAAACACT-1\n";
        let table = read_cells(Cursor::new(later)).unwrap();
        assert_eq!(table.columns, ["cell_type"]);
        assert!(table.has("AAACCCAAGAAACACT-1"));
        let failed = read_cells(Cursor::new("barcode\nAAACCCAAGAAACACT-1\n")).unwrap_err();
        assert!(
            matches!(failed, Error::Invalid(Reason::CellTable)),
            "{failed}"
        );
    }

    fn fields(table: &Table, sample: &str) -> Vec<(String, String)> {
        table.fields(sample)
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn tab_tables_find_the_sample_column_by_its_name() {
        let text = "pop\tsample\tsuper_pop\tgender\t\t\n\
            GBR\tHG00096\tEUR\tmale\t\t\n\
            YRI\tNA18486\tAFR\tfemale\t\t\n";
        let table = opened(text, "integrated_call_samples.panel");
        assert_eq!(table.columns, ["pop", "super_pop", "gender"]);
        assert_eq!(table.len(), 2);
        assert_eq!(
            fields(&table, "NA18486"),
            pairs(&[("pop", "YRI"), ("super_pop", "AFR"), ("gender", "female")])
        );
    }

    #[test]
    fn the_first_column_names_the_samples_without_a_sample_header() {
        let text = "subject,group,age\r\nP1,case,40\r\n\"P2\",\"control, late\",\"55\"\r\n\
            P3,\"say \"\"hi\"\"\",\n";
        let table = opened(text, "cohort.csv");
        assert_eq!(table.columns, ["group", "age"]);
        assert_eq!(
            fields(&table, "P2"),
            pairs(&[("group", "control, late"), ("age", "55")])
        );
        assert_eq!(fields(&table, "P3"), pairs(&[("group", "say \"hi\"")]));
        assert!(table.is_number(1));
    }

    #[test]
    fn igv_files_give_only_their_sample_table() {
        let text = "#sampleTable\nID\tSubtype\tStage\nTCGA-1\tLumA\tII\n\
            #sampleMapping\nTCGA-1\tfile1\n\
            #colors\nSubtype\tLumA\t0,0,255\n";
        let table = opened(text, "attributes.txt");
        assert_eq!(table.columns, ["Subtype", "Stage"]);
        assert_eq!(table.len(), 1);
        assert_eq!(
            fields(&table, "TCGA-1"),
            pairs(&[("Subtype", "LumA"), ("Stage", "II")])
        );
        let marked = opened("#sample\tpop\n# a note\nS1\tEUR\n", "groups.tsv");
        assert_eq!(marked.columns, ["pop"]);
        assert_eq!(fields(&marked, "S1"), pairs(&[("pop", "EUR")]));
    }

    /// cBioPortal clinical files start with four # lines: the names to show, the descriptions,
    /// the types and the priorities. The line after them is the header.
    #[test]
    fn cbioportal_files_take_the_header_after_their_four_lines_and_show_their_names() {
        let text = "#Patient Identifier\tSample Identifier\tCancer Type\tTMB (nonsynonymous)\n\
            #Identifier of a patient.\tA unique sample identifier.\tCancer type.\tTMB.\n\
            #STRING\tSTRING\tSTRING\tNUMBER\n\
            #1\t1\t1\t1\n\
            PATIENT_ID\tSAMPLE_ID\tCANCER_TYPE\tTMB_NONSYNONYMOUS\n\
            TCGA-OR-A5J1\tTCGA-OR-A5J1-01\tAdrenocortical Carcinoma\t0.6\n\
            TCGA-OR-A5J2\tTCGA-OR-A5J2-01\tAdrenocortical Carcinoma\t1.1\n";
        let table = opened(text, "data_clinical_sample.txt");
        assert_eq!(table.len(), 2);
        assert_eq!(
            table.columns,
            ["Patient Identifier", "Cancer Type", "TMB (nonsynonymous)"]
        );
        assert_eq!(
            fields(&table, "TCGA-OR-A5J1-01"),
            pairs(&[
                ("Patient Identifier", "TCGA-OR-A5J1"),
                ("Cancer Type", "Adrenocortical Carcinoma"),
                ("TMB (nonsynonymous)", "0.6")
            ])
        );
        assert!(table.is_number(2));
    }

    #[test]
    fn plink_files_give_family_parents_sex_and_phenotype() {
        assert!(is_plink("trio.fam") && is_plink("study.PED") && is_plink("study.ped.gz"));
        assert!(!is_plink("samples.tsv"));
        let text = "F1 I1 0 0 1 2\nF1 I2 I1 0 2 1\nF2\tI3\t0\t0\t0\t-9\n";
        let table = opened(text, "trio.fam");
        assert_eq!(
            table.columns,
            ["Family", "Father", "Mother", "Sex", "Phenotype"]
        );
        assert_eq!(
            fields(&table, "I1"),
            pairs(&[("Family", "F1"), ("Sex", "male"), ("Phenotype", "affected")])
        );
        assert_eq!(
            fields(&table, "I2"),
            pairs(&[
                ("Family", "F1"),
                ("Father", "I1"),
                ("Sex", "female"),
                ("Phenotype", "unaffected")
            ])
        );
        assert_eq!(fields(&table, "I3"), pairs(&[("Family", "F2")]));
        let ped = opened(
            "F I1 0 0 1 3.5 A A G T\nF I2 0 0 2 -9 A C G G\n",
            "trait.ped",
        );
        assert_eq!(
            fields(&ped, "I1"),
            pairs(&[("Family", "F"), ("Sex", "male"), ("Phenotype", "3.5")])
        );
        assert_eq!(
            fields(&ped, "I2"),
            pairs(&[("Family", "F"), ("Sex", "female")])
        );
        assert!(ped.is_number(4));
    }

    #[test]
    fn broken_tables_say_why() {
        let failure = |text: &str, name: &str| match read(Cursor::new(text), name) {
            Err(Error::Format { line, reason }) => (line, reason),
            Err(Error::Invalid(reason)) => (0, reason),
            other => panic!("expected an error, got {other:?}"),
        };
        assert_eq!(failure("", "empty.tsv"), (0, Reason::SampleTable));
        assert_eq!(
            failure("sample\nS1\n", "names.tsv"),
            (0, Reason::SampleTable)
        );
        assert_eq!(
            failure("sample\tpop\n", "header.tsv"),
            (0, Reason::SampleTable)
        );
        assert_eq!(
            failure("F1 I1 0 0\n", "short.fam"),
            (1, Reason::MissingField)
        );
    }
}
