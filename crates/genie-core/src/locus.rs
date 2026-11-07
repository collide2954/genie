use std::fmt;
use std::str::FromStr;

use crate::{Region, grouped, parse_bases};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locus {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    Whole,
    Position(u64),
    Region(Region),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocusError {
    Empty,
    BadNumber(String),
    ZeroPosition,
    Reversed,
    UnknownContig(String),
    PastEnd { name: String, length: u64 },
}

impl FromStr for Locus {
    type Err = LocusError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.trim();
        let (name, span) = match text.rsplit_once(':') {
            Some((name, spec)) => (name.trim(), parse_span(spec.trim())?),
            None => (text, Span::Whole),
        };
        if name.is_empty() {
            return Err(LocusError::Empty);
        }
        Ok(Self {
            name: name.to_owned(),
            span,
        })
    }
}

impl fmt::Display for Locus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = &self.name;
        match self.span {
            Span::Whole => f.write_str(name),
            Span::Position(position) => write!(f, "{name}:{}", grouped(position.saturating_add(1))),
            Span::Region(region) => write!(
                f,
                "{name}:{}-{}",
                grouped(region.start().saturating_add(1)),
                grouped(region.end())
            ),
        }
    }
}

fn parse_span(spec: &str) -> Result<Span, LocusError> {
    if spec.is_empty() {
        return Ok(Span::Whole);
    }
    match spec.split_once('-') {
        Some((start, end)) => Region::new(position(start)?, number(end)?)
            .filter(|region| !region.is_empty())
            .map(Span::Region)
            .ok_or(LocusError::Reversed),
        None => position(spec).map(Span::Position),
    }
}

fn number(text: &str) -> Result<u64, LocusError> {
    let text = text.trim();
    parse_bases(text).ok_or_else(|| LocusError::BadNumber(text.to_owned()))
}

fn position(text: &str) -> Result<u64, LocusError> {
    number(text)?.checked_sub(1).ok_or(LocusError::ZeroPosition)
}

impl fmt::Display for LocusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("The locus is empty."),
            Self::BadNumber(text) => write!(f, "\"{text}\" is not a position."),
            Self::ZeroPosition => f.write_str("Positions start at 1."),
            Self::Reversed => f.write_str("The start is after the end."),
            Self::UnknownContig(name) => write!(f, "The assembly has no contig \"{name}\"."),
            Self::PastEnd { name, length } => write!(f, "{name} ends at {}.", grouped(*length)),
        }
    }
}

impl std::error::Error for LocusError {}

#[cfg(test)]
mod tests {
    use super::{Locus, LocusError, Span};
    use crate::Region;

    fn parse(text: &str) -> Result<Locus, LocusError> {
        text.parse()
    }

    fn locus(name: &str, span: Span) -> Locus {
        Locus {
            name: name.to_owned(),
            span,
        }
    }

    fn range(start: u64, end: u64) -> Span {
        Span::Region(Region::new(start, end).unwrap())
    }

    #[test]
    fn name_alone_is_whole_contig() {
        assert_eq!(parse("  chr1 "), Ok(locus("chr1", Span::Whole)));
        assert_eq!(parse("chr1:"), Ok(locus("chr1", Span::Whole)));
    }

    #[test]
    fn single_position_is_one_based() {
        assert_eq!(
            parse("chr1:12,345"),
            Ok(locus("chr1", Span::Position(12_344)))
        );
        assert_eq!(parse("chr1:1"), Ok(locus("chr1", Span::Position(0))));
    }

    #[test]
    fn range_is_one_based_inclusive() {
        assert_eq!(
            parse("chr1:1,000-2,000"),
            Ok(locus("chr1", range(999, 2_000)))
        );
        assert_eq!(parse("chr1:5-5"), Ok(locus("chr1", range(4, 5))));
        assert_eq!(
            parse("chr1: 1k - 2.5m "),
            Ok(locus("chr1", range(999, 2_500_000)))
        );
    }

    #[test]
    fn last_colon_separates_name() {
        assert_eq!(
            parse("HLA-A*01:01:7"),
            Ok(locus("HLA-A*01:01", Span::Position(6)))
        );
    }

    #[test]
    fn rejects_invalid_text() {
        assert_eq!(parse("   "), Err(LocusError::Empty));
        assert_eq!(parse(":100"), Err(LocusError::Empty));
        assert_eq!(parse("chr1:abc"), Err(LocusError::BadNumber("abc".into())));
        assert_eq!(parse("chr1:0"), Err(LocusError::ZeroPosition));
        assert_eq!(parse("chr1:0-10"), Err(LocusError::ZeroPosition));
        assert_eq!(parse("chr1:2,000-1,000"), Err(LocusError::Reversed));
        assert_eq!(parse("chr1:1000-999"), Err(LocusError::Reversed));
    }

    #[test]
    fn displays_one_based_grouped_text() {
        assert_eq!(locus("chr1", Span::Whole).to_string(), "chr1");
        assert_eq!(
            locus("chr1", Span::Position(12_344)).to_string(),
            "chr1:12,345"
        );
        assert_eq!(
            locus("chr1", range(999, 2_000)).to_string(),
            "chr1:1,000-2,000"
        );
    }

    #[test]
    fn display_parses_back() {
        for original in [
            locus("chrX", Span::Whole),
            locus("2", Span::Position(0)),
            locus("chr1", range(0, 248_956_422)),
        ] {
            assert_eq!(parse(&original.to_string()), Ok(original));
        }
    }

    #[test]
    fn errors_have_messages() {
        assert_eq!(
            LocusError::ZeroPosition.to_string(),
            "Positions start at 1."
        );
        assert_eq!(
            LocusError::BadNumber("abc".into()).to_string(),
            "\"abc\" is not a position."
        );
    }
}
