#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    pub base: u8,
    pub code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Call {
    pub at: u32,
    pub code: u8,
    pub chance: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Modifications {
    pub codes: Vec<Code>,
    pub calls: Vec<Call>,
}

pub const MODIFIED_FROM: u8 = 128;

pub fn parse(mm: &str, ml: &[u8], bases: &[u8], reverse: bool) -> Modifications {
    calls(mm, ml, bases, reverse).unwrap_or_default()
}

fn complement(base: u8) -> u8 {
    match base {
        b'A' => b'T',
        b'C' => b'G',
        b'G' => b'C',
        b'T' | b'U' => b'A',
        other => other,
    }
}

fn header(text: &str) -> Option<(u8, Vec<String>)> {
    let (&base, rest) = text.as_bytes().split_first()?;
    let (&strand, rest) = rest.split_first()?;
    let base = base.to_ascii_uppercase();
    if !matches!(strand, b'+' | b'-') || !b"ACGTUN".contains(&base) {
        return None;
    }
    let rest = rest
        .strip_suffix(b"?")
        .or_else(|| rest.strip_suffix(b"."))
        .unwrap_or(rest);
    let codes = if !rest.is_empty() && rest.iter().all(u8::is_ascii_digit) {
        vec![String::from_utf8_lossy(rest).into_owned()]
    } else if !rest.is_empty() && rest.iter().all(u8::is_ascii_alphabetic) {
        rest.iter()
            .map(|&letter| char::from(letter).to_string())
            .collect()
    } else {
        return None;
    };
    Some((base, codes))
}

fn calls(mm: &str, ml: &[u8], bases: &[u8], reverse: bool) -> Option<Modifications> {
    let length = bases.len();
    let original = |index: usize| {
        let base = if reverse {
            complement(bases[length - 1 - index].to_ascii_uppercase())
        } else {
            bases[index].to_ascii_uppercase()
        };
        if base == b'U' { b'T' } else { base }
    };
    let mut found = Modifications::default();
    let mut chances = ml.iter();
    for entry in mm.split(';').filter(|entry| !entry.trim().is_empty()) {
        let mut fields = entry.trim().split(',');
        let (base, codes) = header(fields.next()?)?;
        let target = if base == b'U' { b'T' } else { base };
        let (first, count) = (found.codes.len(), codes.len());
        found
            .codes
            .extend(codes.into_iter().map(|code| Code { base, code }));
        let mut index = 0;
        for delta in fields {
            let skip: usize = delta.trim().parse().ok()?;
            let mut seen = 0;
            let at = loop {
                if index >= length {
                    return None;
                }
                let here = index;
                index += 1;
                if target == b'N' || original(here) == target {
                    if seen == skip {
                        break here;
                    }
                    seen += 1;
                }
            };
            let stored = u32::try_from(if reverse { length - 1 - at } else { at }).ok()?;
            for offset in 0..count {
                found.calls.push(Call {
                    at: stored,
                    code: u8::try_from(first + offset).ok()?,
                    chance: *chances.next()?,
                });
            }
        }
    }
    if chances.next().is_some() {
        return None;
    }
    found.calls.sort_by_key(|call| (call.at, call.code));
    Some(found)
}

pub fn name(code: &str) -> String {
    let known = match code {
        "m" => "5mC",
        "h" => "5hmC",
        "f" => "5fC",
        "c" => "5caC",
        "g" => "5hmU",
        "e" => "5fU",
        "b" => "5caU",
        "a" => "6mA",
        "o" => "8oxoG",
        "n" => "Xao",
        _ if !code.is_empty() && code.bytes().all(|byte| byte.is_ascii_digit()) => {
            return format!("ChEBI {code}");
        }
        _ => code,
    };
    known.to_owned()
}

impl Modifications {
    pub fn counts(&self) -> Vec<(String, usize, usize)> {
        let mut counts: Vec<(String, usize, usize)> = Vec::new();
        for call in &self.calls {
            let Some(code) = self.codes.get(usize::from(call.code)) else {
                continue;
            };
            let name = name(&code.code);
            let place = counts
                .iter()
                .position(|(known, _, _)| *known == name)
                .unwrap_or_else(|| {
                    counts.push((name, 0, 0));
                    counts.len() - 1
                });
            counts[place].2 += 1;
            if call.chance >= MODIFIED_FROM {
                counts[place].1 += 1;
            }
        }
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::{Call, Code, Modifications, name, parse};

    fn call(at: u32, code: u8, chance: u8) -> Call {
        Call { at, code, chance }
    }

    fn code(base: u8, code: &str) -> Code {
        Code {
            base,
            code: code.to_owned(),
        }
    }

    #[test]
    fn calls_skip_bases_of_their_kind() {
        let found = parse("C+m?,0,1;", &[200, 30], b"ACGTCGACCG", false);
        assert_eq!(
            found,
            Modifications {
                codes: vec![code(b'C', "m")],
                calls: vec![call(1, 0, 200), call(7, 0, 30)],
            }
        );
    }

    #[test]
    fn codes_in_one_entry_share_positions() {
        let found = parse("C+mh,0;C+76792.,2;", &[180, 20, 99], b"ACGTCGACCG", false);
        assert_eq!(
            found.codes,
            [code(b'C', "m"), code(b'C', "h"), code(b'C', "76792")]
        );
        assert_eq!(
            found.calls,
            [call(1, 0, 180), call(1, 1, 20), call(7, 2, 99)]
        );
    }

    #[test]
    fn reverse_reads_count_on_the_original_strand() {
        let found = parse("C+m,1;N+n,2;", &[250, 10], b"ACGTCGACCG", true);
        assert_eq!(found.calls, [call(5, 0, 250), call(7, 1, 10)]);
    }

    #[test]
    fn bad_tags_give_no_calls() {
        let empty = Modifications::default();
        assert_eq!(parse("C+m,9;", &[1], b"ACGT", false), empty);
        assert_eq!(parse("C+m,0;", &[], b"ACGT", false), empty);
        assert_eq!(parse("C+m,0;", &[1, 2], b"ACGT", false), empty);
        assert_eq!(parse("Cm,0;", &[1], b"ACGT", false), empty);
        assert_eq!(parse("C+m,x;", &[1], b"ACGT", false), empty);
        assert_eq!(parse("", &[], b"ACGT", false), empty);
    }

    #[test]
    fn counts_name_the_modifications() {
        let found = parse("C+mh,0,0;A+a,0;", &[200, 10, 100, 20, 128], b"CCA", false);
        assert_eq!(
            found.counts(),
            [
                ("5mC".to_owned(), 1, 2),
                ("5hmC".to_owned(), 0, 2),
                ("6mA".to_owned(), 1, 1)
            ]
        );
        assert_eq!(name("76792"), "ChEBI 76792");
        assert_eq!(name("q"), "q");
    }
}
