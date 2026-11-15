//! The type, the length and the ends of the structural variants in VCF records.

use genie_core::Region;
use genie_core::variant::{Structure, Sv};

/// The structure of a record that is a structural variant: a record with SVTYPE, with a
/// symbolic allele such as `<DEL>`, or with a breakend such as `G]17:198982]`.
pub fn structure(reference: &str, alternate: &str, info: &str, span: Region) -> Option<Structure> {
    let value = |key: &str| {
        info.split(';')
            .find_map(|entry| entry.strip_prefix(key)?.strip_prefix('='))
    };
    let symbol = alternate
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'));
    let kind = value("SVTYPE")
        .and_then(kind_of)
        .or_else(|| symbol.and_then(kind_of))
        .or_else(|| alternate.contains(['[', ']']).then_some(Sv::Breakend))?;
    let mate = (kind == Sv::Breakend)
        .then(|| {
            mate(alternate).or_else(|| {
                let position: u64 = value("POS2").or_else(|| value("END"))?.parse().ok()?;
                Some((value("CHR2")?.to_owned(), position.checked_sub(1)?))
            })
        })
        .flatten();
    let length = value("SVLEN")
        .and_then(|text| text.split(',').next()?.trim().parse::<i64>().ok())
        .map(i64::unsigned_abs)
        .or_else(|| match kind {
            Sv::Breakend => None,
            _ if symbol.is_some() => span.len().checked_sub(1),
            Sv::Insertion | Sv::Deletion => {
                Some((alternate.len() as u64).abs_diff(reference.len() as u64))
            }
            _ => Some(span.len()),
        })
        .filter(|length| *length > 0);
    Some(Structure {
        kind,
        length,
        mate,
        around: [value("CIPOS"), value("CIEND")].map(|text| text.and_then(interval)),
        precise: !info.split(';').any(|entry| entry == "IMPRECISE"),
    })
}

fn kind_of(text: &str) -> Option<Sv> {
    let text = text.to_ascii_uppercase();
    let main = text.split(':').next().unwrap_or_default();
    let copies = main
        .strip_prefix("CN")
        .is_some_and(|count| !count.is_empty() && count.bytes().all(|byte| byte.is_ascii_digit()));
    Some(match main {
        "DEL" => Sv::Deletion,
        "DUP" => Sv::Duplication,
        "INV" => Sv::Inversion,
        "INS" => Sv::Insertion,
        "CNV" => Sv::CopyNumber,
        "BND" | "TRA" => Sv::Breakend,
        _ if copies => Sv::CopyNumber,
        _ => return None,
    })
}

/// The contig and the position (from 0) of the other end of a breakend, from its alternate
/// allele, as in `G]17:198982]` or `[chr2:321682[T`.
fn mate(alternate: &str) -> Option<(String, u64)> {
    let start = alternate.find(['[', ']'])?;
    let bracket = char::from(alternate.as_bytes()[start]);
    let rest = &alternate[start + 1..];
    let end = rest.find(bracket)?;
    let (contig, position) = rest[..end].rsplit_once(':')?;
    let contig = contig.trim_start_matches('<').trim_end_matches('>');
    let position: u64 = position.parse().ok()?;
    Some((contig.to_owned(), position.checked_sub(1)?))
}

fn interval(text: &str) -> Option<(i64, i64)> {
    let (low, high) = text.split_once(',')?;
    Some((low.trim().parse().ok()?, high.trim().parse().ok()?))
}

#[cfg(test)]
mod tests {
    use genie_core::Region;
    use genie_core::variant::{Structure, Sv};

    use super::structure;

    fn sv(reference: &str, alternate: &str, info: &str, span: (u64, u64)) -> Option<Structure> {
        structure(reference, alternate, info, Region::between(span.0, span.1))
    }

    #[test]
    fn deletions_give_their_length_and_their_uncertain_ends() {
        let info = "PRECISE;SVTYPE=DEL;SVLEN=-490;END=500;SUPPORT=12;CIPOS=-5,5;CIEND=-10,12";
        assert_eq!(
            sv("N", "<DEL>", info, (9, 500)),
            Some(Structure {
                kind: Sv::Deletion,
                length: Some(490),
                mate: None,
                around: [Some((-5, 5)), Some((-10, 12))],
                precise: true,
            })
        );
        let tandem = sv(
            "A",
            "<DUP:TANDEM>",
            "SVTYPE=DUP;END=2000;IMPRECISE",
            (999, 2000),
        );
        let tandem = tandem.unwrap();
        assert_eq!(
            (tandem.kind, tandem.length, tandem.precise),
            (Sv::Duplication, Some(1000), false)
        );
        let inversion = sv("A", "<INV>", "END=300", (99, 300)).unwrap();
        assert_eq!(
            (inversion.kind, inversion.length),
            (Sv::Inversion, Some(200))
        );
        assert_eq!(inversion.around, [None, None]);
    }

    #[test]
    fn insertions_and_deletions_with_bases_count_their_bases() {
        let inserted = format!("A{}", "C".repeat(312));
        let pbsv = sv("A", &inserted, "SVTYPE=INS;SVLEN=312", (9, 10)).unwrap();
        assert_eq!((pbsv.kind, pbsv.length), (Sv::Insertion, Some(312)));
        let bare = sv("A", &inserted, "SVTYPE=INS", (9, 10)).unwrap();
        assert_eq!(bare.length, Some(312));
        let removed = sv(&"G".repeat(61), "G", "SVTYPE=DEL", (9, 70)).unwrap();
        assert_eq!((removed.kind, removed.length), (Sv::Deletion, Some(60)));
        let mobile = sv("T", "<INS:ME:ALU>", "SVLEN=281", (4, 5)).unwrap();
        assert_eq!((mobile.kind, mobile.length), (Sv::Insertion, Some(281)));
    }

    #[test]
    fn breakends_know_their_other_end() {
        let joined = |alternate: &str, info: &str| {
            let found = sv("G", alternate, info, (9, 10)).unwrap();
            assert_eq!((found.kind, found.length), (Sv::Breakend, None));
            found.mate
        };
        let mate = |contig: &str, position: u64| Some((contig.to_owned(), position));
        assert_eq!(
            joined("G]17:198982]", "SVTYPE=BND;MATEID=bnd_2"),
            mate("17", 198_981)
        );
        assert_eq!(
            joined("[chr2:321682[T", "SVTYPE=BND"),
            mate("chr2", 321_681)
        );
        assert_eq!(joined("T[chr5:100[", "."), mate("chr5", 99));
        assert_eq!(
            joined("N]HLA-A*01:01:01:01:1500]", "SVTYPE=BND"),
            mate("HLA-A*01:01:01:01", 1499)
        );
        assert_eq!(joined("C[<ctg1>:7[", "SVTYPE=BND"), mate("ctg1", 6));
        assert_eq!(
            joined("<BND>", "SVTYPE=BND;CHR2=chr5;END=1000"),
            mate("chr5", 999)
        );
        assert_eq!(joined("<TRA>", "CHR2=chr5;POS2=7"), mate("chr5", 6));
        assert_eq!(joined("G.", "SVTYPE=BND"), None);
    }

    #[test]
    fn copy_numbers_and_small_variants() {
        let lost = sv("A", "<CN0>", "END=5000", (999, 5000)).unwrap();
        assert_eq!((lost.kind, lost.length), (Sv::CopyNumber, Some(4000)));
        assert_eq!(
            sv("A", "<CNV>", "SVTYPE=CNV", (9, 10)).unwrap().length,
            None
        );
        assert_eq!(sv("A", "G", "DP=3", (9, 10)), None);
        assert_eq!(sv("A", "<NON_REF>", ".", (9, 10)), None);
        assert_eq!(sv("A", "*", ".", (9, 10)), None);
        let deletion = sv("AC", "A", "SVTYPE=DEL;END=11", (9, 11)).unwrap();
        assert_eq!(deletion.mate, None);
    }
}
