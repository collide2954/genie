use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Format { line: u64, reason: Reason },
    Invalid(Reason),
    Remote(String),
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
    NotTabix,
    BadIndex,
    BadSpan,
    BadStrand,
    BadBlocks,
    NotBai,
    NotBam,
    BadRecord,
    NotBigWig,
    NotGzip,
    Unsorted,
    Scattered,
    TooLong,
    NotCram,
    CramVersion,
    BadCram,
    CramOldCodec,
    CramNewCodec,
    CramReference,
    SegmentHeader,
    SampleTable,
    NotHic,
    HicVersion,
    NotBcf,
    BadBcf,
    NotHdf5,
    BadHdf5,
    Hdf5Feature,
    NotCooler,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // The end of a cut file, without words of its own: the standard library says only
            // "failed to fill whole buffer".
            Self::Io(error)
                if error.kind() == io::ErrorKind::UnexpectedEof && error.get_ref().is_none() =>
            {
                f.write_str(
                    "The file stops before the end of its data. Make sure that the file is complete.",
                )
            }
            Self::Io(error) => error.fmt(f),
            Self::Format { line, reason } => write!(f, "Line {line}: {reason}."),
            Self::Remote(message) => f.write_str(message),
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
            Self::NotTabix => "the file is not a tabix index",
            Self::BadIndex => "the index is not valid",
            Self::BadSpan => "the start is after the end",
            Self::BadStrand => "the strand is not +, - or .",
            Self::BadBlocks => "the number of blocks is not the same as the block count",
            Self::NotBai => "the file is not a BAI index",
            Self::NotBam => "the file is not a BAM file",
            Self::BadRecord => "a BAM record is not valid",
            Self::NotBigWig => "the file is not a bigWig or bigBed file",
            Self::NotGzip => "the file is not compressed with gzip",
            Self::Unsorted => "the records are not sorted by position",
            Self::Scattered => "the records of one contig are not together",
            Self::TooLong => "a position is too large for this kind of index",
            Self::NotCram => "the file is not a CRAM file",
            Self::CramVersion => "genie reads CRAM version 3.0 and 3.1 files",
            Self::BadCram => "a CRAM block is not valid",
            Self::CramOldCodec => {
                "the file uses bzip2 or lzma compression, which genie does not read"
            }
            Self::CramNewCodec => {
                "the file uses CRAM 3.1 arithmetic, fqzcomp or name tokenizer compression, which genie does not read yet"
            }
            Self::CramReference => "the reference is not the reference of the CRAM file here",
            Self::SegmentHeader => {
                "the header line does not name the chromosome, start and end columns"
            }
            Self::SampleTable => {
                "the file has no header line with sample names and fields, or no samples"
            }
            Self::NotHic => "the file is not a Hi-C file of Juicer",
            Self::HicVersion => "genie reads Hi-C files of version 6 to 9",
            Self::NotBcf => "the file is not a BCF file of version 2",
            Self::BadBcf => "a BCF record is not valid",
            Self::NotHdf5 => "the file is not an HDF5 file",
            Self::BadHdf5 => "a part of the HDF5 file is not valid",
            Self::Hdf5Feature => "the HDF5 file uses a storage that genie cannot read",
            Self::NotCooler => "the file is not a cooler file of Hi-C contacts",
        })
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Format { .. } | Self::Invalid(_) | Self::Remote(_) => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        match error.downcast::<Self>() {
            Ok(error) => error,
            Err(error) => Self::Io(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read};

    use super::Error;

    const CUT: &str =
        "The file stops before the end of its data. Make sure that the file is complete.";

    /// A file that is cut short gives words that say so, not the words of the standard library.
    #[test]
    fn files_that_stop_too_soon_say_so() {
        let mut bytes = [0; 8];
        let short = (&b"BAM\x01"[..]).read_exact(&mut bytes).unwrap_err();
        assert_eq!(Error::from(short).to_string(), CUT);
        let plain = io::Error::from(io::ErrorKind::UnexpectedEof);
        assert_eq!(Error::from(plain).to_string(), CUT);
        let own = io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "The server sent 10 bytes of 20.",
        );
        assert_eq!(
            Error::from(own).to_string(),
            "The server sent 10 bytes of 20."
        );
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        let words = denied.to_string();
        assert_eq!(Error::from(denied).to_string(), words);
    }
}
