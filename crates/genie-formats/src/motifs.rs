//! Files of DNA motifs: the MEME format, the JASPAR formats with and without brackets, and
//! the HOMER format.

use genie_core::motif::Motif;

use crate::{Error, Reason};

/// The motifs of a file, and the chance of each base (A, C, G, T) in the background.
#[derive(Debug, Clone, PartialEq)]
pub struct MotifSet {
    pub motifs: Vec<Motif>,
    pub background: [f64; 4],
}

const EVEN: [f64; 4] = [0.25; 4];

/// True for the names of motif files: `.meme`, `.jaspar`, `.pfm` and `.motif`.
pub fn is_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".gz").unwrap_or(&lower);
    stem.rsplit_once('.').is_some_and(|(_, extension)| {
        matches!(extension, "meme" | "jaspar" | "pfm" | "motif" | "motifs")
    })
}

/// True when the first lines of a text file are the start of a MEME or JASPAR file, as the
/// files that JASPAR gives with the extension `.txt`.
pub fn sniff(lines: &[String]) -> bool {
    let mut lines = lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty());
    let Some(first) = lines.next() else {
        return false;
    };
    first.starts_with("MEME version")
        || (first.starts_with('>') && lines.next().is_some_and(|second| row(second).is_some()))
}

/// The motifs of a MEME, JASPAR or HOMER file.
pub fn read(text: &str) -> Result<MotifSet, Error> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let first = lines
        .iter()
        .find(|line| !line.is_empty())
        .copied()
        .unwrap_or_default();
    let set = if first.starts_with("MEME version") {
        meme(&lines)?
    } else if first.starts_with('>') && is_homer(first) {
        homer(&lines)?
    } else if first.starts_with('>') {
        jaspar(&lines)?
    } else {
        return Err(Error::Invalid(Reason::NotMotifs));
    };
    if set.motifs.is_empty() {
        return Err(Error::Invalid(Reason::NotMotifs));
    }
    Ok(set)
}

/// A row of numbers of a matrix, with the base at its start in JASPAR files, as
/// `A  [ 4  19  0 ]`, `4 19 0` or `0.1 0.7 0.1 0.1`.
fn row(line: &str) -> Option<(Option<usize>, Vec<f64>)> {
    let line = line.trim();
    let mut letters = line.chars();
    let base = match (letters.next()?, letters.next()) {
        (letter @ ('A' | 'C' | 'G' | 'T' | 'U'), Some(next))
            if next == '[' || next.is_whitespace() =>
        {
            Some(match letter {
                'A' => 0,
                'C' => 1,
                'G' => 2,
                _ => 3,
            })
        }
        _ => None,
    };
    let numbers = if base.is_some() { &line[1..] } else { line };
    let numbers: Vec<f64> = numbers
        .replace(['[', ']'], " ")
        .split_whitespace()
        .map(|word| word.parse().ok())
        .collect::<Option<_>>()?;
    (!numbers.is_empty()).then_some((base, numbers))
}

fn meme(lines: &[&str]) -> Result<MotifSet, Error> {
    let mut background = EVEN;
    let mut motifs = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        at += 1;
        if let Some(alphabet) = line.strip_prefix("ALPHABET=") {
            let alphabet = alphabet.trim();
            if !alphabet.starts_with("ACGT") && !alphabet.starts_with("ACGU") {
                return Err(Error::Invalid(Reason::NotMotifs));
            }
        } else if line.starts_with("Background letter frequencies") {
            let mut found = [None; 4];
            while at < lines.len()
                && found.iter().any(Option::is_none)
                && !lines[at].starts_with("MOTIF")
            {
                let words: Vec<&str> = lines[at].split_whitespace().collect();
                for pair in words.chunks(2) {
                    if let [letter, value] = pair
                        && let Some(place) = "ACGTU".find(*letter).map(|place| place.min(3))
                    {
                        found[place] = value.parse::<f64>().ok();
                    }
                }
                at += 1;
            }
            if let [Some(a), Some(c), Some(g), Some(t)] = found {
                background = even_out([a, c, g, t]);
            }
        } else if let Some(rest) = line.strip_prefix("MOTIF") {
            let mut words = rest.split_whitespace();
            let id = words.next().unwrap_or_default().to_owned();
            let name = words.next().map_or_else(|| id.clone(), str::to_owned);
            while at < lines.len() && !lines[at].starts_with("letter-probability matrix") {
                at += 1;
            }
            let Some(header) = lines.get(at) else {
                break;
            };
            let values = settings(header);
            at += 1;
            let width = values
                .iter()
                .find(|(key, _)| key == "w")
                .and_then(|(_, value)| value.parse::<usize>().ok());
            let mut probabilities = Vec::new();
            while at < lines.len() && width.is_none_or(|width| probabilities.len() < width) {
                let Some((None, numbers)) = row(lines[at]) else {
                    break;
                };
                let row: [f64; 4] = numbers
                    .try_into()
                    .map_err(|_| Error::Invalid(Reason::BadMotif))?;
                probabilities.push(row);
                at += 1;
            }
            if probabilities.is_empty() {
                return Err(Error::Invalid(Reason::BadMotif));
            }
            let sites = values
                .iter()
                .find(|(key, _)| key == "nsites")
                .and_then(|(_, value)| value.parse().ok());
            motifs.push(Motif {
                id,
                name,
                probabilities,
                sites,
            });
        }
    }
    Ok(MotifSet { motifs, background })
}

/// The `key= value` pairs of a MEME line, as `alength= 4 w= 19 nsites= 20 E= 0`.
fn settings(line: &str) -> Vec<(String, String)> {
    let words: Vec<&str> = line
        .split_once(':')
        .map_or(line, |(_, rest)| rest)
        .split_whitespace()
        .collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at < words.len() {
        if let Some((key, value)) = words[at].split_once('=') {
            if value.is_empty() {
                found.push((
                    key.to_owned(),
                    words.get(at + 1).copied().unwrap_or_default().to_owned(),
                ));
                at += 1;
            } else {
                found.push((key.to_owned(), value.to_owned()));
            }
        }
        at += 1;
    }
    found
}

/// The chances of a background, made to add up to one when they are off.
fn even_out(chances: [f64; 4]) -> [f64; 4] {
    let total: f64 = chances.iter().sum();
    if total <= 0.0 {
        return EVEN;
    }
    if (total - 1.0).abs() < 1e-3 {
        return chances;
    }
    chances.map(|chance| chance / total)
}

/// True for the header of a HOMER motif: the consensus, the name and the threshold, between
/// tabs.
fn is_homer(header: &str) -> bool {
    let fields: Vec<&str> = header.split('\t').collect();
    fields.len() >= 3 && fields[2].trim().parse::<f64>().is_ok()
}

fn jaspar(lines: &[&str]) -> Result<MotifSet, Error> {
    let mut motifs = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let Some(header) = lines[at].strip_prefix('>') else {
            at += 1;
            continue;
        };
        at += 1;
        let mut words = header.split_whitespace();
        let id = words.next().unwrap_or_default().to_owned();
        let rest: Vec<&str> = words.collect();
        let name = if rest.is_empty() {
            id.clone()
        } else {
            rest.join(" ")
        };
        let mut rows: Vec<(Option<usize>, Vec<f64>)> = Vec::new();
        while rows.len() < 4 && at < lines.len() && !lines[at].starts_with('>') {
            if let Some(found) = row(lines[at]) {
                rows.push(found);
            }
            at += 1;
        }
        let width = rows.first().map_or(0, |(_, numbers)| numbers.len());
        if rows.len() != 4 || width == 0 || rows.iter().any(|(_, numbers)| numbers.len() != width) {
            return Err(Error::Invalid(Reason::BadMotif));
        }
        let mut counts = [const { Vec::new() }; 4];
        for (place, (base, numbers)) in rows.into_iter().enumerate() {
            counts[base.unwrap_or(place)] = numbers;
        }
        let mut probabilities = Vec::with_capacity(width);
        let mut sites: f64 = 0.0;
        for column in 0..width {
            let values: [f64; 4] =
                std::array::from_fn(|base| counts[base].get(column).copied().unwrap_or(0.0));
            let total: f64 = values.iter().sum();
            sites = sites.max(total);
            probabilities.push(if total > 0.0 {
                values.map(|value| value / total)
            } else {
                EVEN
            });
        }
        motifs.push(Motif {
            id,
            name,
            probabilities,
            sites: (sites > 0.0).then_some(sites),
        });
    }
    Ok(MotifSet {
        motifs,
        background: EVEN,
    })
}

fn homer(lines: &[&str]) -> Result<MotifSet, Error> {
    let mut motifs = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let Some(header) = lines[at].strip_prefix('>') else {
            at += 1;
            continue;
        };
        at += 1;
        let fields: Vec<&str> = header.split('\t').collect();
        let id = fields
            .get(1)
            .copied()
            .unwrap_or(fields[0])
            .trim()
            .to_owned();
        let name = id.split('/').next().unwrap_or(&id).to_owned();
        let mut probabilities = Vec::new();
        while at < lines.len() && !lines[at].starts_with('>') {
            if let Some((None, numbers)) = row(lines[at]) {
                let row: [f64; 4] = numbers
                    .try_into()
                    .map_err(|_| Error::Invalid(Reason::BadMotif))?;
                probabilities.push(row);
            }
            at += 1;
        }
        if probabilities.is_empty() {
            return Err(Error::Invalid(Reason::BadMotif));
        }
        motifs.push(Motif {
            id,
            name,
            probabilities,
            sites: None,
        });
    }
    Ok(MotifSet {
        motifs,
        background: EVEN,
    })
}

#[cfg(test)]
mod tests {
    use super::{is_name, read, sniff};
    use crate::{Error, Reason};

    const MEME: &str = "\
MEME version 4

ALPHABET= ACGT

strands: + -

Background letter frequencies
A 0.3 C 0.2 G 0.2
T 0.3

MOTIF MA0139.1 CTCF
letter-probability matrix: alength= 4 w= 3 nsites= 10 E= 0
 0.1 0.7 0.1 0.1
 0.0 0.0 1.0 0.0
  0.25 0.25 0.25 0.25
URL http://jaspar.genereg.net/matrix/MA0139.1

MOTIF M2
letter-probability matrix: alength= 4 w= 2
 1 0 0 0
 0 0 0 1
";

    const JASPAR: &str = "\
>MA0004.1\tArnt
A  [ 4  19  0  0 ]
C  [16   0 20  0 ]
G  [ 0   1  0 20 ]
T  [ 0   0  0  0 ]
>MA0006.1 Ahr::Arnt
A [ 3 0 ]
C [ 8 0 ]
G [ 2 23 ]
T [ 11 1 ]
";

    const PFM: &str = "\
>MA0004.1 Arnt
4 19 0 0
16 0 20 0
0 1 0 20
0 0 0 0
";

    const HOMER: &str = "\
>CACGTG\tArnt:Ahr(bHLH)/MCF7-Arnt-ChIP-Seq(Lo_et_al.)/Homer\t6.469\t-1.2\t0
0.003\t0.996\t0.001\t0.001
0.997\t0.001\t0.001\t0.001
>GGAA\tETS/Homer\t5.0
0.1\t0.1\t0.7\t0.1
0.1\t0.1\t0.7\t0.1
";

    fn close(found: &[[f64; 4]], wanted: &[[f64; 4]]) -> bool {
        found.len() == wanted.len()
            && found
                .iter()
                .zip(wanted)
                .all(|(a, b)| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9))
    }

    #[test]
    fn names_and_heads_tell_motif_files() {
        for name in ["a.meme", "b.jaspar", "c.PFM", "d.motif", "e.meme.gz"] {
            assert!(is_name(name), "{name}");
        }
        for name in ["a.txt", "a.bed", "motif"] {
            assert!(!is_name(name), "{name}");
        }
        let lines = |text: &str| text.lines().map(str::to_owned).collect::<Vec<_>>();
        assert!(sniff(&lines(MEME)));
        assert!(sniff(&lines(JASPAR)));
        assert!(sniff(&lines(PFM)));
        assert!(!sniff(&lines("chr1\t10\t20\n")));
        assert!(!sniff(&lines(">chr1\nACGTACGT\n")));
    }

    #[test]
    fn meme_files_give_motifs_and_their_background() {
        let set = read(MEME).unwrap();
        assert_eq!(set.background, [0.3, 0.2, 0.2, 0.3]);
        let ids: Vec<(&str, &str)> = set
            .motifs
            .iter()
            .map(|motif| (motif.id.as_str(), motif.name.as_str()))
            .collect();
        assert_eq!(ids, [("MA0139.1", "CTCF"), ("M2", "M2")]);
        assert!(close(
            &set.motifs[0].probabilities,
            &[[0.1, 0.7, 0.1, 0.1], [0.0, 0.0, 1.0, 0.0], [0.25; 4]]
        ));
        assert_eq!(set.motifs[0].sites, Some(10.0));
        assert_eq!(set.motifs[1].sites, None);
        assert_eq!(set.motifs[1].consensus(), "AT");
    }

    #[test]
    fn jaspar_files_give_counts_as_chances() {
        for text in [JASPAR, PFM] {
            let set = read(text).unwrap();
            assert_eq!(set.background, [0.25; 4]);
            assert_eq!(set.motifs[0].id, "MA0004.1");
            assert_eq!(set.motifs[0].name, "Arnt");
            assert_eq!(set.motifs[0].sites, Some(20.0));
            assert!(close(
                &set.motifs[0].probabilities,
                &[
                    [0.2, 0.8, 0.0, 0.0],
                    [0.95, 0.0, 0.05, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0]
                ]
            ));
        }
        let set = read(JASPAR).unwrap();
        assert_eq!(set.motifs[1].name, "Ahr::Arnt");
        assert_eq!(set.motifs[1].consensus(), "TG");
    }

    #[test]
    fn homer_files_give_rows_of_chances() {
        let set = read(HOMER).unwrap();
        let ids: Vec<(&str, &str)> = set
            .motifs
            .iter()
            .map(|motif| (motif.id.as_str(), motif.name.as_str()))
            .collect();
        assert_eq!(
            ids,
            [
                (
                    "Arnt:Ahr(bHLH)/MCF7-Arnt-ChIP-Seq(Lo_et_al.)/Homer",
                    "Arnt:Ahr(bHLH)"
                ),
                ("ETS/Homer", "ETS")
            ]
        );
        assert_eq!(set.motifs[0].consensus(), "CA");
        assert_eq!(set.motifs[1].probabilities.len(), 2);
    }

    #[test]
    fn other_files_are_not_motif_files() {
        for text in [
            "",
            "chr1\t10\t20\n",
            "MEME version 4\nALPHABET= ACDEFGHIKLMNPQRSTVWY\n",
        ] {
            let error = read(text).unwrap_err();
            assert!(
                matches!(error, Error::Invalid(Reason::NotMotifs)),
                "{text}: {error}"
            );
        }
        let error = read(">M1 x\nA [ 1 2 ]\nC [ 1 ]\nG [ 1 2 ]\nT [ 1 2 ]\n").unwrap_err();
        assert!(matches!(error, Error::Invalid(Reason::BadMotif)), "{error}");
    }
}
