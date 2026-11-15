use std::io::{BufRead, Read, Seek};
use std::str::Split;

use genie_core::Region;
use genie_core::variant::{Call, Variant};

use crate::annotations::{self, Layout};
use crate::bgzf::Bgzf;
use crate::features::Entry;
use crate::fields::{self, Field};
use crate::tabix::Index;
use crate::table::Table;
use crate::text::lines;
use crate::{Error, Reason};

const INFO_LIMIT: usize = 400;
const MORE: &str = "…";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    pub samples: Vec<String>,
    pub infos: Vec<(String, String)>,
}

pub fn fields(info: &str, infos: &[(String, String)]) -> Vec<Field> {
    let labels: Vec<String> = infos
        .iter()
        .map(|(id, about)| fields::label(about, id))
        .collect();
    let label_of = |key: &str| {
        let Some(at) = infos.iter().position(|(id, _)| id == key) else {
            return key.to_owned();
        };
        let label = &labels[at];
        let shared = labels
            .iter()
            .enumerate()
            .any(|(other, text)| other != at && text == label);
        if shared {
            key.to_owned()
        } else {
            label.clone()
        }
    };
    let mut found = Vec::new();
    for entry in info.split(';') {
        if entry == MORE {
            found.push(Field {
                label: "More INFO".to_owned(),
                value: format!("Cut at {INFO_LIMIT} characters"),
            });
            continue;
        }
        let (key, value) = entry.split_once('=').unwrap_or((entry, "yes"));
        if matches!(key, "" | ".") || !fields::shown(value) {
            continue;
        }
        found.push(Field {
            label: label_of(key),
            value: fields::plain(value),
        });
    }
    found
}

fn info_line(line: &str) -> Option<(String, String)> {
    let body = line.strip_prefix("##INFO=<")?;
    let id = body
        .strip_prefix("ID=")
        .or_else(|| body.split_once(",ID=").map(|(_, rest)| rest))?;
    let id = id.split([',', '>']).next()?.to_owned();
    let about = body
        .split_once("Description=\"")
        .map_or_else(String::new, |(_, rest)| quoted(rest));
    Some((id, about))
}

fn quoted(text: &str) -> String {
    let mut found = String::new();
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => found.extend(characters.next()),
            '"' => break,
            _ => found.push(character),
        }
    }
    found
}

pub fn header(mut reader: impl BufRead) -> Result<Header, Error> {
    let mut bytes = Vec::new();
    let mut infos = Vec::new();
    loop {
        bytes.clear();
        if reader.read_until(b'\n', &mut bytes)? == 0 {
            return Ok(Header {
                samples: Vec::new(),
                infos,
            });
        }
        let text = String::from_utf8_lossy(&bytes);
        let line = text.trim_end_matches(['\n', '\r']);
        if line.starts_with("##") {
            infos.extend(info_line(line));
            continue;
        }
        let samples = line
            .strip_prefix('#')
            .map(|columns| columns.split('\t').skip(9).map(str::to_owned).collect())
            .unwrap_or_default();
        return Ok(Header { samples, infos });
    }
}

/// The contig and the variant of a record. The annotations of the `layouts` leave the INFO and
/// go into the variant.
pub fn parse<'a>(line: &'a str, layouts: &[Layout]) -> Result<Option<(&'a str, Variant)>, Reason> {
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let mut fields = line.split('\t');
    let contig = field(&mut fields)?;
    let position: u64 = field(&mut fields)?.parse().map_err(|_| Reason::BadNumber)?;
    let id = field(&mut fields)?;
    let reference = field(&mut fields)?;
    let alternates = field(&mut fields)?;
    let quality = match field(&mut fields)? {
        "." => None,
        text => Some(
            text.parse::<f32>()
                .ok()
                .filter(|quality| quality.is_finite())
                .ok_or(Reason::BadNumber)?,
        ),
    };
    let filter = field(&mut fields)?;
    let info = field(&mut fields)?;
    let calls = match fields.next() {
        Some(format) => {
            let at = format.split(':').position(|key| key == "GT");
            fields
                .map(|sample| {
                    at.and_then(|at| sample.split(':').nth(at))
                        .map_or(Call::Missing, Call::parse)
                })
                .collect()
        }
        None => Vec::new(),
    };
    let start = position.saturating_sub(1);
    let (annotations, kept) = annotations::split(info, layouts);
    Ok(Some((
        contig,
        Variant {
            id: if id == "." {
                String::new()
            } else {
                id.to_owned()
            },
            span: extent(start, reference, info),
            reference: reference.to_owned(),
            alternates: alternates.split(',').map(str::to_owned).collect(),
            quality,
            filter: filter.to_owned(),
            info: cut(&kept),
            annotations,
            calls,
        },
    )))
}

pub fn query<R: Read + Seek>(
    index: &Index,
    reader: &mut Bgzf<R>,
    contig: &str,
    region: Region,
    layouts: &[Layout],
) -> Result<Vec<Variant>, Error> {
    let mut variants = Vec::new();
    index.query(reader, contig, region, |line| {
        if let Some((_, variant)) = parse(line, layouts).map_err(Error::Invalid)? {
            variants.push(variant);
        }
        Ok(())
    })?;
    Ok(variants)
}

pub fn read_table(reader: impl BufRead) -> Result<(Header, Table<Variant>), Error> {
    let mut samples = Vec::new();
    let mut infos = Vec::new();
    let mut layouts = None;
    let mut rows = Vec::new();
    lines(reader, |number, line| {
        if line.starts_with("##") {
            infos.extend(info_line(line));
            return Ok(());
        }
        if line.starts_with('#') {
            samples = line.split('\t').skip(9).map(str::to_owned).collect();
            return Ok(());
        }
        let layouts = layouts.get_or_insert_with(|| annotations::layouts(&infos));
        let parsed = parse(line, layouts).map_err(|reason| Error::Format {
            line: number,
            reason,
        })?;
        if let Some((contig, variant)) = parsed {
            rows.push((contig.to_owned(), variant.span, variant));
        }
        Ok(())
    })?;
    Ok((Header { samples, infos }, Table::new(rows)))
}

pub fn names(reader: impl BufRead) -> Result<Vec<Entry>, Error> {
    let mut entries = Vec::new();
    lines(reader, |_, line| {
        if line.starts_with('#') {
            return Ok(());
        }
        let mut fields = line.split('\t');
        let (Some(contig), Some(position), Some(ids), Some(reference)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return Ok(());
        };
        let Ok(position) = position.parse::<u64>() else {
            return Ok(());
        };
        let start = position.saturating_sub(1);
        let span = Region::between(start, start + (reference.len() as u64).max(1));
        for id in ids.split(';').filter(|id| !id.is_empty() && *id != ".") {
            entries.push(Entry {
                name: id.to_owned(),
                contig: contig.to_owned(),
                span,
            });
        }
        Ok(())
    })?;
    Ok(entries)
}

pub(crate) fn extent(start: u64, reference: &str, info: &str) -> Region {
    let end = info
        .split(';')
        .find_map(|entry| entry.strip_prefix("END="))
        .and_then(|value| value.parse::<u64>().ok());
    let end = match end {
        Some(end) if end > start => end,
        Some(_) => start + 1,
        None => start + (reference.len() as u64).max(1),
    };
    Region::between(start, end)
}

fn field<'a>(fields: &mut Split<'a, char>) -> Result<&'a str, Reason> {
    fields.next().ok_or(Reason::MissingField)
}

fn cut(info: &str) -> String {
    if info.len() <= INFO_LIMIT {
        return info.to_owned();
    }
    let at = info.as_bytes()[..INFO_LIMIT]
        .iter()
        .rposition(|byte| *byte == b';');
    let end = at.unwrap_or_else(|| {
        let mut end = INFO_LIMIT;
        while !info.is_char_boundary(end) {
            end -= 1;
        }
        end
    });
    format!("{};{MORE}", &info[..end])
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use genie_core::Region;
    use genie_core::variant::Call;

    use super::{extent, fields, header, names, parse, query, read_table};
    use crate::bgzf::Bgzf;
    use crate::bgzf::tests::{block, file};
    use crate::fields::Field;
    use crate::tabix::tests::{Bins, VCF, index};
    use crate::{Error, Reason};

    #[test]
    fn parses_sites_with_genotypes() {
        let line = "chr1\t100\trs1\tA\tG,T\t50.5\tPASS\tDP=10;AF=0.5\tDP:GT\t12:0/1\t9:1/1\t0:./.";
        let (contig, variant) = parse(line, &[]).unwrap().unwrap();
        assert_eq!(contig, "chr1");
        assert_eq!(variant.span, Region::between(99, 100));
        assert_eq!(variant.id, "rs1");
        assert_eq!(variant.reference, "A");
        assert_eq!(variant.alternates, ["G", "T"]);
        assert_eq!(variant.quality, Some(50.5));
        assert_eq!(variant.filter, "PASS");
        assert_eq!(variant.info, "DP=10;AF=0.5");
        assert_eq!(variant.calls, [Call::Het, Call::Alternate, Call::Missing]);
    }

    #[test]
    fn sites_without_samples_or_values() {
        let (_, variant) = parse("chr2\t5\t.\tC\tT\t.\t.\t.", &[]).unwrap().unwrap();
        assert_eq!(variant.id, "");
        assert_eq!(variant.quality, None);
        assert_eq!(variant.calls, []);
    }

    #[test]
    fn deletions_and_symbolic_alleles_span_their_length() {
        assert_eq!(extent(9, "ACGT", "."), Region::between(9, 13));
        assert_eq!(
            extent(9, "A", "SVTYPE=DEL;END=500"),
            Region::between(9, 500)
        );
        assert_eq!(extent(9, "A", "END=3"), Region::between(9, 10));
        let (_, variant) = parse("chr1\t10\t.\tA\t<DEL>\t.\t.\tEND=500", &[])
            .unwrap()
            .unwrap();
        assert_eq!(variant.span, Region::between(9, 500));
    }

    #[test]
    fn skips_headers_and_reports_bad_lines() {
        assert_eq!(parse("#CHROM\tPOS", &[]), Ok(None));
        assert_eq!(parse("", &[]), Ok(None));
        assert_eq!(
            parse("chr1\tx\t.\tA\tG\t.\t.\t.", &[]),
            Err(Reason::BadNumber)
        );
        for quality in ["low", "nan", "NaN", "inf", "-infinity"] {
            let line = format!("chr1\t10\t.\tA\tG\t{quality}\t.\t.");
            assert_eq!(parse(&line, &[]), Err(Reason::BadNumber), "{quality}");
        }
        assert_eq!(parse("chr1\t10\t.\tA", &[]), Err(Reason::MissingField));
    }

    #[test]
    fn annotations_leave_the_info_and_go_into_the_variant() {
        let csq = "C|intron_variant|MODIFIER|GENEB,".repeat(30);
        let text = format!(
            "##fileformat=VCFv4.2\n\
             ##INFO=<ID=CSQ,Number=.,Type=String,Description=\"Consequence annotations from \
             Ensembl VEP. Format: Allele|Consequence|IMPACT|SYMBOL\">\n\
             #CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\n\
             chr1\t10\t.\tT\tC\t.\tPASS\tDP=4;CSQ={csq}C|stop_gained|HIGH|GENEA;AF=0.5\n"
        );
        let (header, table) = read_table(Cursor::new(text.as_bytes())).unwrap();
        let found = table.query("chr1", Region::between(0, 100));
        let variant = &found[0];
        assert_eq!(variant.info, "DP=4;AF=0.5");
        let first = &variant.annotations.first;
        assert_eq!((first[0].gene.as_str(), first.len()), ("GENEA", 3));
        assert_eq!(variant.annotations.others, 28);
        let layouts = crate::annotations::layouts(&header.infos);
        let line = text.lines().last().unwrap();
        let (_, parsed) = parse(line, &layouts).unwrap().unwrap();
        assert_eq!(&parsed, variant);
        let (_, bare) = parse(line, &[]).unwrap().unwrap();
        assert_eq!(bare.info, "DP=4;…");
        assert_eq!(bare.annotations.first, []);
    }

    #[test]
    fn long_info_is_cut_at_a_field() {
        let info = (0..200)
            .map(|index| format!("K{index}=1"))
            .collect::<Vec<_>>()
            .join(";");
        let line = format!("chr1\t10\t.\tA\tG\t.\t.\t{info}");
        let (_, variant) = parse(&line, &[]).unwrap().unwrap();
        let kept = variant.info.strip_suffix(";…").unwrap();
        assert!(kept.len() <= 400, "{}", kept.len());
        assert!(kept.ends_with('1'), "{kept}");
        assert!(kept.starts_with("K0=1;K1=1"));
        let shown = pairs(fields(&variant.info, &[]));
        assert_eq!(
            shown.last().unwrap(),
            &("More INFO".to_owned(), "Cut at 400 characters".to_owned())
        );
    }

    fn pairs(fields: Vec<Field>) -> Vec<(String, String)> {
        fields
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect()
    }

    const INFOS: &str = "##fileformat=VCFv4.2\n\
        ##INFO=<ID=DP,Number=1,Type=Integer,Description=\"Total depth, all samples\">\n\
        ##INFO=<ID=DB,Number=0,Type=Flag,Description=\"dbSNP membership\">\n\
        ##INFO=<ID=NOTE,Number=1,Type=String,Description=\"Note with \\\"quotes\\\"\",Source=\"x\">\n\
        #CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\n\
        chr1\t5\t.\tC\tT\t.\tPASS\tDP=14;DB\n";

    #[test]
    fn info_fields_take_their_labels_from_the_header() {
        let expected = [
            ("DP", "Total depth, all samples"),
            ("DB", "dbSNP membership"),
            ("NOTE", "Note with \"quotes\""),
        ];
        let expected: Vec<(String, String)> = expected
            .iter()
            .map(|(id, about)| ((*id).to_owned(), (*about).to_owned()))
            .collect();
        let found = header(INFOS.as_bytes()).unwrap();
        assert_eq!(found.infos, expected);
        let (table_header, _) = read_table(INFOS.as_bytes()).unwrap();
        assert_eq!(table_header.infos, expected);
        let shown = pairs(fields("DP=14;DB;AF=.;XX=1;.", &found.infos));
        let wanted = [
            ("Total depth", "14"),
            ("dbSNP membership", "yes"),
            ("XX", "1"),
        ];
        let wanted: Vec<(String, String)> = wanted
            .iter()
            .map(|(label, value)| ((*label).to_owned(), (*value).to_owned()))
            .collect();
        assert_eq!(shown, wanted);
    }

    /// Long descriptions can give two fields the same short label, as in the INFO of the 1000
    /// Genomes Project. Those fields show their ID, in each card the same.
    #[test]
    fn info_fields_with_the_same_short_label_show_their_ids() {
        let infos: Vec<(String, String)> = [
            ("AF_EUR", "Allele frequency in EUR"),
            (
                "AF_EUR_unrel",
                "Allele frequency in EUR (2504 unrelated samples)",
            ),
            (
                "AN_EAS",
                "Total number of alleles in called genotypes in EAS",
            ),
            (
                "AN_AMR",
                "Total number of alleles in called genotypes in AMR",
            ),
            ("DP", "Read depth"),
        ]
        .iter()
        .map(|(id, about)| ((*id).to_owned(), (*about).to_owned()))
        .collect();
        let shown = pairs(fields("AF_EUR=0.1;AF_EUR_unrel=0.2;AN_EAS=10;DP=5", &infos));
        let wanted = [
            ("AF_EUR", "0.1"),
            ("AF_EUR_unrel", "0.2"),
            ("AN_EAS", "10"),
            ("Read depth", "5"),
        ];
        let wanted: Vec<(String, String)> = wanted
            .iter()
            .map(|(label, value)| ((*label).to_owned(), (*value).to_owned()))
            .collect();
        assert_eq!(shown, wanted);
    }

    const TEXT: &str = "##fileformat=VCFv4.2\n\
        #CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\tFORMAT\tNA1\n\
        chr1\t50\trs5\tA\tG\t.\tPASS\t.\tGT\t0/1\n\
        chr1\t10\trs1;rs2\tACGT\tA\t.\tPASS\t.\tGT\t1/1\n\
        chr2\t5\t.\tC\tT\t.\tPASS\t.\tGT\t0/0\n";

    #[test]
    fn tables_hold_variants_and_samples() {
        let (header, table) = read_table(TEXT.as_bytes()).unwrap();
        assert_eq!(header.samples, ["NA1"]);
        assert_eq!(table.contigs(), ["chr1", "chr2"]);
        let found = table.query("chr1", Region::between(11, 60));
        let ids: Vec<&str> = found.iter().map(|variant| variant.id.as_str()).collect();
        assert_eq!(ids, ["rs1;rs2", "rs5"]);
        let bad = format!("{TEXT}chr2\tx\t.\tC\tT\t.\t.\t.\n");
        assert!(matches!(
            read_table(bad.as_bytes()),
            Err(Error::Format { line: 6, .. })
        ));
    }

    #[test]
    fn names_list_variant_ids() {
        let entries = names(TEXT.as_bytes()).unwrap();
        let found: Vec<(&str, &str, Region)> = entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.contig.as_str(), entry.span))
            .collect();
        assert_eq!(
            found,
            [
                ("rs5", "chr1", Region::between(49, 50)),
                ("rs1", "chr1", Region::between(9, 13)),
                ("rs2", "chr1", Region::between(9, 13)),
            ]
        );
    }

    #[test]
    fn queries_variants_through_an_index() {
        let data = TEXT.lines().skip(2).filter(|line| line.starts_with("chr1"));
        let mut sorted: Vec<&str> = data.collect();
        sorted.reverse();
        let text: String = sorted.iter().flat_map(|line| [*line, "\n"]).collect();
        let end = (block(text.as_bytes()).len() as u64) << 16;
        let bins: Bins = &[(4681, &[(0, end)])];
        let index = index(VCF, &["chr1"], &[(bins, &[0])]);
        let mut reader = Bgzf::new(Cursor::new(file(&[text.as_bytes()])));
        let found = query(&index, &mut reader, "chr1", Region::between(12, 20), &[]).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].span, Region::between(9, 13));
    }

    #[test]
    fn header_lists_samples() {
        let text = "##fileformat=VCFv4.2\n##contig=<ID=chr1>\n\
                    #CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\tFORMAT\tNA1\tNA2\n\
                    chr1\t1\t.\tA\tG\t.\t.\t.\tGT\t0/1\t0/0\n";
        assert_eq!(header(text.as_bytes()).unwrap().samples, ["NA1", "NA2"]);
        let sites = "##fileformat=VCFv4.2\n#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\n";
        assert_eq!(header(sites.as_bytes()).unwrap().samples, [] as [String; 0]);
    }
}
