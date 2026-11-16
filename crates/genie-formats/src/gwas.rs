//! GWAS summary statistics: tables with the chromosome, the position and the p-value of each
//! variant, as the GWAS Catalog, PLINK 2, REGENIE, SAIGE and BOLT-LMM write them.

use std::io::{BufRead, Read, Seek};

use genie_core::Region;

use crate::Error;
use crate::bgzf::Bgzf;
use crate::fields::{self, Field};
use crate::tabix::Index;

const CHROMOSOMES: [&str; 6] = [
    "chromosome",
    "hm_chrom",
    "chrom",
    "chr",
    "chr_name",
    "chromosome_name",
];
/// The names of the position column. `BP` comes before `GENPOS`, because `GENPOS` of BOLT-LMM
/// is a genetic position, while `GENPOS` of REGENIE is in bases.
const POSITIONS: [&str; 7] = [
    "base_pair_location",
    "hm_pos",
    "pos",
    "bp",
    "genpos",
    "position",
    "base_pair",
];
const P_VALUES: [&str; 7] = [
    "p_value",
    "p",
    "pval",
    "p.value",
    "p_bolt_lmm",
    "p_bolt_lmm_inf",
    "pvalue",
];
const LOG10: [&str; 4] = ["neg_log_10_p_value", "log10p", "log10_p", "mlog10p"];
/// The -log10 p of a p-value of 0 or below the smallest number.
const HIGHEST: f32 = 320.0;

/// The columns of a GWAS table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub names: Vec<String>,
    chromosome: usize,
    position: usize,
    p: Option<usize>,
    log10: Option<usize>,
    /// Whether spaces, not tabs, separate the fields, as in REGENIE files.
    spaces: bool,
}

impl Layout {
    /// The layout of a header line, if it names a chromosome, a position and a p-value.
    pub fn sniff(header: &str) -> Option<Self> {
        let line = header.trim_end_matches(['\n', '\r']);
        let spaces = !line.contains('\t');
        let names: Vec<String> = split(line, spaces)
            .into_iter()
            .map(|name| name.trim_start_matches('#').to_owned())
            .collect();
        let find = |wanted: &[&str]| {
            wanted.iter().find_map(|name| {
                names
                    .iter()
                    .position(|found| found.eq_ignore_ascii_case(name))
            })
        };
        let (p, log10) = (find(&P_VALUES), find(&LOG10));
        if p.is_none() && log10.is_none() {
            return None;
        }
        Some(Self {
            chromosome: find(&CHROMOSOMES)?,
            position: find(&POSITIONS)?,
            p,
            log10,
            spaces,
            names,
        })
    }

    pub fn split<'a>(&self, line: &'a str) -> Vec<&'a str> {
        split(line, self.spaces)
    }

    /// The contig, the position (from 0) and the -log10 p of a row.
    pub fn row<'a>(&self, line: &'a str) -> Option<(&'a str, u64, f32)> {
        let fields = self.split(line);
        let contig = *fields.get(self.chromosome)?;
        let position: u64 = fields.get(self.position)?.trim().parse().ok()?;
        let given = self
            .log10
            .and_then(|column| fields.get(column)?.trim().parse::<f32>().ok());
        let value = given.or_else(|| {
            let p: f64 = fields.get(self.p?)?.trim().parse().ok()?;
            if !(0.0..=1.0).contains(&p) {
                return None;
            }
            Some(if p > 0.0 {
                (-p.log10()) as f32
            } else {
                HIGHEST
            })
        });
        let value = value
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| value.min(HIGHEST))?;
        Some((contig, position.checked_sub(1)?, value))
    }

    /// The fields of a row for its card: each column with its name, except the chromosome and
    /// the position.
    pub fn fields(&self, row: &str) -> Vec<Field> {
        self.split(row)
            .into_iter()
            .enumerate()
            .filter(|(column, value)| {
                ![self.chromosome, self.position].contains(column) && fields::shown(value)
            })
            .map(|(column, value)| Field {
                label: self
                    .names
                    .get(column)
                    .cloned()
                    .unwrap_or_else(|| format!("Column {}", column + 1)),
                value: value.to_owned(),
            })
            .collect()
    }
}

fn split(line: &str, spaces: bool) -> Vec<&str> {
    if spaces {
        line.split_ascii_whitespace().collect()
    } else {
        line.split('\t').collect()
    }
}

/// The header line of a file: its first line that is not empty and does not start with `##`.
pub fn header(mut reader: impl BufRead) -> Result<Option<String>, Error> {
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        if reader.read_until(b'\n', &mut bytes)? == 0 {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&bytes);
        let line = text.trim_end_matches(['\n', '\r']);
        if !line.trim().is_empty() && !line.starts_with("##") {
            return Ok(Some(line.to_owned()));
        }
    }
}

/// Calls `each` with the position (from 0), the -log10 p and the text of each row of a region,
/// with the tabix index of the file.
pub fn query<R: Read + Seek>(
    index: &Index,
    reader: &mut Bgzf<R>,
    layout: &Layout,
    (contig, region): (&str, Region),
    mut each: impl FnMut(u64, f32, &str),
) -> Result<(), Error> {
    index.query(reader, contig, region, |line| {
        if let Some((_, position, value)) = layout.row(line) {
            each(position, value, line);
        }
        Ok(())
    })
}

/// Reads a whole file: gives its layout, and calls `each` with the contig, the position (from
/// 0), the -log10 p and the text of each row. Without a header that names a chromosome, a
/// position and a p-value, it gives nothing.
pub fn read(
    mut reader: impl BufRead,
    mut each: impl FnMut(&str, u64, f32, &str),
) -> Result<Option<Layout>, Error> {
    let Some(layout) = header(&mut reader)?.as_deref().and_then(Layout::sniff) else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        if reader.read_until(b'\n', &mut bytes)? == 0 {
            return Ok(Some(layout));
        }
        let text = String::from_utf8_lossy(&bytes);
        let line = text.trim_end_matches(['\n', '\r']);
        if let Some((contig, position, value)) = layout.row(line) {
            each(contig, position, value, line);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use genie_core::Region;

    use super::{Layout, header, query, read};
    use crate::bgzf::{self, Bgzf};
    use crate::tabix::{self, Index};

    const CATALOG: &str = "chromosome\tbase_pair_location\teffect_allele\tother_allele\tbeta\t\
        standard_error\teffect_allele_frequency\tp_value\tvariant_id\trsid\n\
        1\t1000\tA\tG\t0.1\t0.01\t0.3\t5e-9\t1_1000_G_A\trs1\n\
        1\t2000\tC\tT\t-0.02\t0.01\t0.4\t0.2\t1_2000_T_C\trs2\n\
        2\t500\tG\tA\t0.3\t0.05\t0.1\t0\t2_500_A_G\trs3\n";

    fn close(value: f32, wanted: f32) -> bool {
        (value - wanted).abs() < 1e-4
    }

    #[test]
    fn headers_of_the_common_tools_name_the_columns() {
        let catalog = Layout::sniff(CATALOG.lines().next().unwrap()).unwrap();
        let (contig, position, value) = catalog.row(CATALOG.lines().nth(1).unwrap()).unwrap();
        assert_eq!((contig, position), ("1", 999));
        assert!(close(value, 8.301_03), "{value}");
        let zero = catalog.row(CATALOG.lines().nth(3).unwrap()).unwrap();
        assert!(close(zero.2, 320.0));
        let plink =
            Layout::sniff("#CHROM\tPOS\tID\tREF\tALT\tA1\tTEST\tOBS_CT\tBETA\tSE\tT_STAT\tP")
                .unwrap();
        let row = plink
            .row("3\t52\trs9\tA\tG\tG\tADD\t1000\t0.2\t0.04\t5.0\t1e-6")
            .unwrap();
        assert_eq!((row.0, row.1), ("3", 51));
        assert!(close(row.2, 6.0));
        let regenie = Layout::sniff(
            "CHROM GENPOS ID ALLELE0 ALLELE1 A1FREQ N TEST BETA SE CHISQ LOG10P EXTRA",
        )
        .unwrap();
        let row = regenie
            .row("1 79033 1:79033 A G 0.0016 500 ADD 0.1 0.2 0.5 7.5 NA")
            .unwrap();
        assert_eq!(row, ("1", 79_032, 7.5));
        let bolt = Layout::sniff(
            "SNP\tCHR\tBP\tGENPOS\tALLELE1\tALLELE0\tA1FREQ\tINFO\tBETA\tSE\tP_BOLT_LMM_INF\t\
             P_BOLT_LMM",
        )
        .unwrap();
        let row = bolt
            .row("rs5\t1\t4000\t0.012\tA\tC\t0.2\t0.99\t0.1\t0.02\t1e-3\t1e-4")
            .unwrap();
        assert_eq!((row.0, row.1), ("1", 3_999));
        assert!(close(row.2, 4.0));
        let saige = Layout::sniff("CHR\tPOS\tMarkerID\tAllele1\tAllele2\tBETA\tSE\tp.value");
        assert!(saige.is_some());
        assert_eq!(Layout::sniff("sample\tpop\tage"), None);
        assert_eq!(catalog.row("1\tx\tA\tG\t0\t0\t0\t0.5\tid\trs"), None);
        assert_eq!(catalog.row("1\t10\tA\tG\t0\t0\t0\t2\tid\trs"), None);
    }

    #[test]
    fn cards_name_the_other_columns() {
        let catalog = Layout::sniff(CATALOG.lines().next().unwrap()).unwrap();
        let fields = catalog.fields(CATALOG.lines().nth(1).unwrap());
        let labels: Vec<&str> = fields.iter().map(|field| field.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "effect_allele",
                "other_allele",
                "beta",
                "standard_error",
                "effect_allele_frequency",
                "p_value",
                "variant_id",
                "rsid"
            ]
        );
    }

    #[test]
    fn indexed_files_give_the_rows_of_a_region() {
        let mut writer = bgzf::Writer::new(Vec::new());
        writer.write_all(CATALOG.as_bytes()).unwrap();
        let data = writer.finish().unwrap();
        let layout = tabix::Layout::generic([1, 2, 2]).skipping(1);
        let index = tabix::build(Cursor::new(&data), layout, false).unwrap();
        let index = Index::read(Cursor::new(index)).unwrap();
        assert_eq!(index.skip(), 1);
        let mut reader = Bgzf::new(Cursor::new(data.clone()));
        let found = header(&mut reader).unwrap().unwrap();
        let layout = Layout::sniff(&found).unwrap();
        let mut rows = Vec::new();
        query(
            &index,
            &mut reader,
            &layout,
            ("1", Region::between(0, 1_500)),
            |position, value, line| {
                rows.push((position, value > 8.0, line.ends_with("rs1")));
            },
        )
        .unwrap();
        assert_eq!(rows, [(999, true, true)]);
        let mut all = Vec::new();
        let read_layout = read(Bgzf::new(Cursor::new(data)), |contig, position, _, _| {
            all.push((contig.to_owned(), position));
        })
        .unwrap();
        assert_eq!(read_layout, Some(layout));
        assert_eq!(
            all,
            [
                ("1".to_owned(), 999),
                ("1".to_owned(), 1_999),
                ("2".to_owned(), 499)
            ]
        );
        assert_eq!(
            read(Cursor::new("sample\tpop\nS1\tEUR\n"), |_, _, _, _| {}).unwrap(),
            None
        );
    }
}
