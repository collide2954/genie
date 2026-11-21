//! The haplotype of a read from the phased heterozygous SNVs of a sample that it covers, as
//! whatshap haplotag finds it: each base of the read at such an SNV votes for the haplotype that
//! has the base.

use crate::alignment::{Alignment, Op};

/// The least quality of a base that votes.
pub const LEAST_QUALITY: u8 = 20;
/// The fewest votes that give a read a haplotype.
pub const LEAST_VOTES: u32 = 2;

/// A phased heterozygous SNV of a sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Phased {
    /// The place of the SNV, from 0.
    pub position: u64,
    /// The base of haplotype 1 and the base of haplotype 2, in upper case.
    pub bases: [u8; 2],
    /// The phase set of the SNV (PS), or 0 without one.
    pub set: u64,
}

/// The haplotype of a read, 1 or 2, from the phased SNVs that it covers, in the order of their
/// places: the haplotype of `phase_of`.
pub fn haplotype(alignment: &Alignment, sites: &[Phased]) -> Option<i64> {
    phase_of(alignment, sites).map(|(_, haplotype)| haplotype)
}

/// The phase set and the haplotype of a read, 1 or 2, from the phased SNVs that it covers, in
/// the order of their places. Each base of the read with a quality of `LEAST_QUALITY` or more
/// votes for the haplotype with that base, in each phase set apart. The set with the most votes
/// decides: the read gets its haplotype with more votes when it has `LEAST_VOTES` votes or more
/// and twice the votes of the other. Else the read has no haplotype.
pub fn phase_of(alignment: &Alignment, sites: &[Phased]) -> Option<(u64, i64)> {
    let mut votes: Vec<(u64, [u32; 2])> = Vec::new();
    let (mut at, mut index) = (alignment.start, 0_usize);
    for &(op, length) in &alignment.cigar {
        let size = u64::from(length);
        match op {
            Op::Match | Op::Equal | Op::Diff => {
                let first = sites.partition_point(|site| site.position < at);
                let inside = sites[first..]
                    .iter()
                    .take_while(|site| site.position < at + size);
                for site in inside {
                    let offset = index + (site.position - at) as usize;
                    let Some(&base) = alignment.bases.get(offset) else {
                        continue;
                    };
                    let quality = alignment.qualities.get(offset).copied();
                    if quality.unwrap_or(u8::MAX) < LEAST_QUALITY {
                        continue;
                    }
                    let base = base.to_ascii_uppercase();
                    let Some(haplotype) = site.bases.iter().position(|&known| known == base) else {
                        continue;
                    };
                    if let Some((_, counts)) = votes.iter_mut().find(|(set, _)| *set == site.set) {
                        counts[haplotype] += 1;
                    } else {
                        let mut counts = [0, 0];
                        counts[haplotype] = 1;
                        votes.push((site.set, counts));
                    }
                }
                at += size;
                index += length as usize;
            }
            Op::Insertion | Op::SoftClip => index += length as usize,
            Op::Deletion | Op::Skip => at += size,
            Op::HardClip | Op::Padding => {}
        }
    }
    let (set, counts) = votes
        .iter()
        .max_by_key(|(_, counts)| counts[0] + counts[1])?;
    let (winner, other, haplotype) = if counts[0] >= counts[1] {
        (counts[0], counts[1], 1)
    } else {
        (counts[1], counts[0], 2)
    };
    (winner >= LEAST_VOTES && winner >= 2 * other && winner > other).then_some((*set, haplotype))
}

/// The reads with an HP tag of 1 or 2 from the file that also get a haplotype from the phased
/// SNVs, in each phase set: the reads whose tag names the same haplotype, and the reads whose tag
/// names the other one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Votes(Vec<(u64, [u32; 2])>);

impl Votes {
    /// Counts a read when it has an HP tag of 1 or 2 from the file and a haplotype from the
    /// phased SNVs.
    pub fn count(&mut self, alignment: &Alignment, sites: &[Phased]) {
        let tagged = match alignment.tags.phase {
            Some(tagged @ (1 | 2)) if !alignment.tags.phase_from_vcf => tagged,
            _ => return,
        };
        let Some((set, found)) = phase_of(alignment, sites) else {
            return;
        };
        let at = self
            .0
            .iter()
            .position(|(known, _)| *known == set)
            .unwrap_or_else(|| {
                self.0.push((set, [0, 0]));
                self.0.len() - 1
            });
        self.0[at].1[usize::from(found != tagged)] += 1;
    }

    /// The phase sets whose SNVs name the haplotypes the other way round from the HP tags: most
    /// of their reads with both disagree.
    pub fn swapped(&self) -> Vec<u64> {
        self.0
            .iter()
            .filter(|(_, [same, other])| other > same)
            .map(|(set, _)| *set)
            .collect()
    }
}

/// Gives a read without a haplotype from an HP tag the haplotype of `phase_of`, with 1 and 2 the
/// other way round in the phase sets of `swapped`, and notes that it comes from the phased SNVs.
/// A read with an HP tag keeps its haplotype. Gives the phase set of the haplotype that the read
/// gets.
pub fn tag(alignment: &mut Alignment, sites: &[Phased], swapped: &[u64]) -> Option<u64> {
    if alignment.tags.phase.is_some() || sites.is_empty() {
        return None;
    }
    let (set, found) = phase_of(alignment, sites)?;
    let haplotype = if swapped.contains(&set) {
        3 - found
    } else {
        found
    };
    alignment.tags.phase = Some(haplotype);
    alignment.tags.phase_from_vcf = true;
    Some(set)
}

#[cfg(test)]
mod tests {
    use super::{Phased, Votes, haplotype, phase_of, tag};
    use crate::alignment::{Alignment, Op, Tags};

    fn read(start: u64, cigar: &[(Op, u32)], bases: &str, low: &[usize]) -> Alignment {
        let mut qualities = vec![30; bases.len()];
        for &at in low {
            qualities[at] = 5;
        }
        Alignment {
            name: "read".to_owned(),
            start,
            flags: 0,
            mapq: 60,
            cigar: cigar.to_vec(),
            bases: bases.as_bytes().to_vec(),
            qualities,
            mate: None,
            tags: Tags::default(),
        }
    }

    fn site(position: u64, first: u8, second: u8, set: u64) -> Phased {
        Phased {
            position,
            bases: [first, second],
            set,
        }
    }

    /// SNVs at 10, 12 and 14 of one phase set. A read from 8 has its bases at 2, 4 and 6.
    fn sites() -> Vec<Phased> {
        vec![
            site(10, b'A', b'G', 5),
            site(12, b'C', b'T', 5),
            site(14, b'G', b'A', 5),
        ]
    }

    #[test]
    fn reads_take_the_haplotype_of_most_of_their_bases() {
        let all = [(Op::Match, 10)];
        assert_eq!(
            haplotype(&read(8, &all, "nnGnTnAnnn", &[]), &sites()),
            Some(2)
        );
        assert_eq!(
            haplotype(&read(8, &all, "nnanCnAnnn", &[]), &sites()),
            Some(1)
        );
        assert_eq!(
            haplotype(&read(8, &all, "nnAnTnAnnn", &[]), &sites()),
            Some(2)
        );
        assert_eq!(
            haplotype(&read(8, &all, "nnAnTnGnnn", &[]), &sites()),
            Some(1)
        );
        assert_eq!(haplotype(&read(8, &all, "nnAnTnnnnn", &[]), &sites()), None);
        assert_eq!(
            haplotype(&read(8, &all, "nnAnCnGnnn", &[2, 4]), &sites()),
            None
        );
        assert_eq!(
            haplotype(&read(13, &all, "nAnnnnnnnn", &[]), &sites()),
            None
        );
    }

    #[test]
    fn clips_and_deletions_move_the_bases_of_the_read() {
        let clipped = [(Op::SoftClip, 3), (Op::Match, 10)];
        assert_eq!(
            haplotype(&read(8, &clipped, "TTTnnGnTnAnnn", &[]), &sites()),
            Some(2)
        );
        let deleted = [(Op::Match, 4), (Op::Deletion, 2), (Op::Match, 4)];
        assert_eq!(
            haplotype(&read(8, &deleted, "nnAnAnnn", &[]), &sites()),
            None
        );
        let both = read(8, &deleted, "nnAnGnnn", &[]);
        assert_eq!(haplotype(&both, &sites()), Some(1));
    }

    #[test]
    fn reads_with_hp_tags_keep_them_and_others_get_the_vcf_haplotype() {
        let all = [(Op::Match, 10)];
        let mut tagged = read(8, &all, "nnGnTnAnnn", &[]);
        tagged.tags.phase = Some(1);
        assert_eq!(tag(&mut tagged, &sites(), &[]), None);
        assert_eq!(
            (tagged.tags.phase, tagged.tags.phase_from_vcf),
            (Some(1), false)
        );
        let mut plain = read(8, &all, "nnGnTnAnnn", &[]);
        assert_eq!(tag(&mut plain, &sites(), &[]), Some(5));
        assert_eq!(
            (plain.tags.phase, plain.tags.phase_from_vcf),
            (Some(2), true)
        );
        let mut unclear = read(8, &all, "nnAnTnnnnn", &[]);
        assert_eq!(tag(&mut unclear, &sites(), &[]), None);
        assert_eq!(
            (unclear.tags.phase, unclear.tags.phase_from_vcf),
            (None, false)
        );
        let mut alone = read(8, &all, "nnGnTnAnnn", &[]);
        assert_eq!(tag(&mut alone, &[], &[]), None);
        assert_eq!(alone.tags.phase, None);
    }

    #[test]
    fn votes_find_the_phase_sets_whose_snvs_name_the_haplotypes_the_other_way_round() {
        let all = [(Op::Match, 10)];
        let tagged = |bases: &str, phase: i64| {
            let mut found = read(8, &all, bases, &[]);
            found.tags.phase = Some(phase);
            found
        };
        let mut votes = Votes::default();
        votes.count(&tagged("nnGnTnAnnn", 2), &sites());
        votes.count(&tagged("nnAnCnGnnn", 2), &sites());
        assert_eq!(votes.swapped(), Vec::<u64>::new());
        votes.count(&tagged("nnAnCnGnnn", 2), &sites());
        assert_eq!(votes.swapped(), [5]);
        let mut from_vcf = tagged("nnAnCnGnnn", 2);
        from_vcf.tags.phase_from_vcf = true;
        for read in [
            from_vcf,
            read(8, &all, "nnGnTnAnnn", &[]),
            tagged("nnGnTnAnnn", 3),
            tagged("nnAnTnnnnn", 1),
        ] {
            votes.count(&read, &sites());
        }
        assert_eq!(votes, {
            let mut expected = Votes::default();
            expected.count(&tagged("nnGnTnAnnn", 2), &sites());
            expected.count(&tagged("nnAnCnGnnn", 2), &sites());
            expected.count(&tagged("nnAnCnGnnn", 2), &sites());
            expected
        });
    }

    #[test]
    fn reads_without_hp_tags_take_the_other_haplotype_in_swapped_phase_sets() {
        let all = [(Op::Match, 10)];
        let mut swapped = read(8, &all, "nnGnTnAnnn", &[]);
        assert_eq!(tag(&mut swapped, &sites(), &[5]), Some(5));
        assert_eq!(
            (swapped.tags.phase, swapped.tags.phase_from_vcf),
            (Some(1), true)
        );
        let mut other_set = read(8, &all, "nnGnTnAnnn", &[]);
        assert_eq!(tag(&mut other_set, &sites(), &[9]), Some(5));
        assert_eq!(other_set.tags.phase, Some(2));
        let mut tagged = read(8, &all, "nnGnTnAnnn", &[]);
        tagged.tags.phase = Some(1);
        assert_eq!(tag(&mut tagged, &sites(), &[5]), None);
        assert_eq!(tagged.tags.phase, Some(1));
        assert_eq!(
            phase_of(&read(8, &all, "nnAnCnGnnn", &[]), &sites()),
            Some((5, 1))
        );
    }

    #[test]
    fn the_phase_set_with_the_most_votes_decides() {
        let mut sites = sites();
        sites.push(site(16, b'T', b'C', 9));
        sites.push(site(17, b'A', b'C', 9));
        sites.push(site(18, b'G', b'C', 9));
        sites.sort_by_key(|site| site.position);
        let all = [(Op::Match, 12)];
        let first = read(8, &all, "nnAnCnAnTAGn", &[]);
        assert_eq!(haplotype(&first, &sites), Some(1));
        let second = read(8, &all, "nnGnTnAnCCCn", &[]);
        assert_eq!(haplotype(&second, &sites), Some(2));
    }
}
