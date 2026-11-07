use std::io::{BufRead, Read, Seek};
use std::str::Split;

use genie_core::Region;
use genie_core::variant::{Call, Variant};

use crate::bgzf::Bgzf;
use crate::features::Entry;
use crate::tabix::Index;
use crate::table::Table;
use crate::{Error, Reason};

const INFO_LIMIT: usize = 400;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    pub samples: Vec<String>,
}

pub fn header(mut reader: impl BufRead) -> Result<Header, Error> {
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        if reader.read_until(b'\n', &mut bytes)? == 0 {
            return Ok(Header::default());
        }
        let text = String::from_utf8_lossy(&bytes);
        let line = text.trim_end_matches(['\n', '\r']);
        if line.starts_with("##") {
            continue;
        }
        let samples = line
            .strip_prefix('#')
            .map(|columns| columns.split('\t').skip(9).map(str::to_owned).collect())
            .unwrap_or_default();
        return Ok(Header { samples });
    }
}

pub fn parse(line: &str) -> Result<Option<(&str, Variant)>, Reason> {
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
        text => Some(text.parse::<f32>().map_err(|_| Reason::BadNumber)?),
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
            info: cut(info).to_owned(),
            calls,
        },
    )))
}

pub fn query<R: Read + Seek>(
    index: &Index,
    reader: &mut Bgzf<R>,
    contig: &str,
    region: Region,
) -> Result<Vec<Variant>, Error> {
    let mut variants = Vec::new();
    index.query(reader, contig, region, |line| {
        if let Some((_, variant)) = parse(line).map_err(Error::Invalid)? {
            variants.push(variant);
        }
        Ok(())
    })?;
    Ok(variants)
}

pub fn read_table(reader: impl BufRead) -> Result<(Header, Table<Variant>), Error> {
    let mut samples = Vec::new();
    let mut rows = Vec::new();
    lines(reader, |number, line| {
        if line.starts_with("##") {
            return Ok(());
        }
        if line.starts_with('#') {
            samples = line.split('\t').skip(9).map(str::to_owned).collect();
            return Ok(());
        }
        let parsed = parse(line).map_err(|reason| Error::Format {
            line: number,
            reason,
        })?;
        if let Some((contig, variant)) = parsed {
            rows.push((contig.to_owned(), variant.span, variant));
        }
        Ok(())
    })?;
    Ok((Header { samples }, Table::new(rows)))
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

fn lines(
    mut reader: impl BufRead,
    mut each: impl FnMut(u64, &str) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut bytes = Vec::new();
    let mut number = 0;
    loop {
        bytes.clear();
        if reader.read_until(b'\n', &mut bytes)? == 0 {
            return Ok(());
        }
        number += 1;
        let text = String::from_utf8_lossy(&bytes);
        let line = text.trim_end_matches(['\n', '\r']);
        if !line.is_empty() {
            each(number, line)?;
        }
    }
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

fn cut(info: &str) -> &str {
    if info.len() <= INFO_LIMIT {
        return info;
    }
    if let Some(at) = info.as_bytes()[..INFO_LIMIT]
        .iter()
        .rposition(|byte| *byte == b';')
    {
        return &info[..at];
    }
    let mut end = INFO_LIMIT;
    while !info.is_char_boundary(end) {
        end -= 1;
    }
    &info[..end]
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use genie_core::Region;
    use genie_core::variant::Call;

    use super::{extent, header, names, parse, query, read_table};
    use crate::bgzf::Bgzf;
    use crate::bgzf::tests::{block, file};
    use crate::tabix::tests::{Bins, VCF, index};
    use crate::{Error, Reason};

    #[test]
    fn parses_sites_with_genotypes() {
        let line = "chr1\t100\trs1\tA\tG,T\t50.5\tPASS\tDP=10;AF=0.5\tDP:GT\t12:0/1\t9:1/1\t0:./.";
        let (contig, variant) = parse(line).unwrap().unwrap();
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
        let (_, variant) = parse("chr2\t5\t.\tC\tT\t.\t.\t.").unwrap().unwrap();
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
        let (_, variant) = parse("chr1\t10\t.\tA\t<DEL>\t.\t.\tEND=500")
            .unwrap()
            .unwrap();
        assert_eq!(variant.span, Region::between(9, 500));
    }

    #[test]
    fn skips_headers_and_reports_bad_lines() {
        assert_eq!(parse("#CHROM\tPOS"), Ok(None));
        assert_eq!(parse(""), Ok(None));
        assert_eq!(parse("chr1\tx\t.\tA\tG\t.\t.\t."), Err(Reason::BadNumber));
        assert_eq!(
            parse("chr1\t10\t.\tA\tG\tlow\t.\t."),
            Err(Reason::BadNumber)
        );
        assert_eq!(parse("chr1\t10\t.\tA"), Err(Reason::MissingField));
    }

    #[test]
    fn long_info_is_cut_at_a_field() {
        let info = (0..200)
            .map(|index| format!("K{index}=1"))
            .collect::<Vec<_>>()
            .join(";");
        let line = format!("chr1\t10\t.\tA\tG\t.\t.\t{info}");
        let (_, variant) = parse(&line).unwrap().unwrap();
        assert!(variant.info.len() <= 400, "{}", variant.info.len());
        assert!(variant.info.ends_with('1'), "{}", variant.info);
        assert!(variant.info.starts_with("K0=1;K1=1"));
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
        let found = query(&index, &mut reader, "chr1", Region::between(12, 20)).unwrap();
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
