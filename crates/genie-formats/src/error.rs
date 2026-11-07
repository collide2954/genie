use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Format { line: u64, reason: Reason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    MissingField,
    BadNumber,
    ShortLineWidth,
    ZeroLineBases,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Format { line, reason } => write!(f, "Line {line}: {reason}."),
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
        })
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Format { .. } => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
