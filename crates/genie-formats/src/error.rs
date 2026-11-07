use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Format { line: u64, reason: Reason },
    Invalid(Reason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    MissingField,
    BadNumber,
    ShortLineWidth,
    ZeroLineBases,
    NoHeader,
    EmptyName,
    UnevenLines,
    BlankLine,
    NotTwoBit,
    UnsupportedVersion,
    BadBlockCount,
    NoSection,
    BadLine,
    NotBgzf,
    BadBlock,
    BadOffset,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Format { line, reason } => write!(f, "Line {line}: {reason}."),
            Self::Invalid(reason) => {
                let reason = reason.to_string();
                let mut letters = reason.chars();
                match letters.next() {
                    Some(first) => write!(f, "{}{}.", first.to_uppercase(), letters.as_str()),
                    None => Ok(()),
                }
            }
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingField => "a field is missing",
            Self::BadNumber => "a number is not valid",
            Self::ShortLineWidth => "the line width is less than the bases per line",
            Self::ZeroLineBases => "the number of bases per line is zero",
            Self::NoHeader => "the sequence has no header line",
            Self::EmptyName => "the header has no name",
            Self::UnevenLines => "the line length is not the same as the first line",
            Self::BlankLine => "a blank line is in the sequence",
            Self::NotTwoBit => "the file is not a 2bit file",
            Self::UnsupportedVersion => "the 2bit version is not supported",
            Self::BadBlockCount => "a block count is larger than the sequence",
            Self::NoSection => "the entry is outside a section",
            Self::BadLine => "the line is not a section or an entry",
            Self::NotBgzf => "the file is not compressed with bgzip",
            Self::BadBlock => "a compressed block is not valid",
            Self::BadOffset => "an offset is not in the file",
        })
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Format { .. } | Self::Invalid(_) => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
