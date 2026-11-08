use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use genie_formats::http::Url;
use genie_formats::remote::Remote;

use crate::paths::file_name;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Location {
    File(PathBuf),
    Url(Url),
}

impl Location {
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if Url::is_url(text) {
            Url::parse(text)
                .map(Self::Url)
                .map_err(|error| error.to_string())
        } else {
            Ok(Self::File(PathBuf::from(text)))
        }
    }

    pub fn name(&self) -> String {
        match self {
            Self::File(path) => file_name(path),
            Self::Url(url) => url.name(),
        }
    }

    pub fn place(&self) -> String {
        match self {
            Self::File(path) => path
                .parent()
                .map(|folder| folder.display().to_string())
                .unwrap_or_default(),
            Self::Url(url) => url.host().to_owned(),
        }
    }

    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Url(_))
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::File(path) => Some(path),
            Self::Url(_) => None,
        }
    }

    #[must_use]
    pub fn with_suffix(&self, suffix: &str) -> Self {
        match self {
            Self::File(path) => {
                let mut text = path.as_os_str().to_owned();
                text.push(suffix);
                Self::File(PathBuf::from(text))
            }
            Self::Url(url) => Self::Url(url.with_suffix(suffix)),
        }
    }

    #[must_use]
    pub fn with_extension(&self, extension: &str) -> Self {
        match self {
            Self::File(path) => Self::File(path.with_extension(extension)),
            Self::Url(url) => Self::Url(url.with_extension(extension)),
        }
    }

    pub fn open(&self) -> Result<Reader, String> {
        match self {
            Self::File(path) => File::open(path)
                .map(Reader::File)
                .map_err(|error| error.to_string()),
            Self::Url(url) => Remote::open(url)
                .map(|remote| Reader::Remote(Box::new(remote)))
                .map_err(|error| error.to_string()),
        }
    }

    pub fn size(&self) -> Result<u64, String> {
        match self {
            Self::File(path) => fs::metadata(path)
                .map(|metadata| metadata.len())
                .map_err(|error| error.to_string()),
            Self::Url(url) => Remote::open(url)
                .map(|remote| remote.size())
                .map_err(|error| error.to_string()),
        }
    }
}

impl From<PathBuf> for Location {
    fn from(path: PathBuf) -> Self {
        Self::File(path)
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(path) => path.display().fmt(f),
            Self::Url(url) => url.fmt(f),
        }
    }
}

#[derive(Debug)]
pub enum Reader {
    File(File),
    Remote(Box<Remote>),
}

impl Read for Reader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::File(file) => file.read(buffer),
            Self::Remote(remote) => remote.read(buffer),
        }
    }
}

impl Seek for Reader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        match self {
            Self::File(file) => file.seek(to),
            Self::Remote(remote) => remote.seek(to),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom};
    use std::path::PathBuf;

    use super::Location;
    use crate::scratch::Scratch;

    #[test]
    fn locations_are_files_or_addresses() {
        let file = Location::parse(" /data/reads.bam ").unwrap();
        assert_eq!(file, Location::File(PathBuf::from("/data/reads.bam")));
        assert_eq!(file.name(), "reads.bam");
        assert!(!file.is_remote());
        assert_eq!(
            file.with_suffix(".bai"),
            Location::File(PathBuf::from("/data/reads.bam.bai"))
        );
        assert_eq!(
            file.with_extension("bai"),
            Location::File(PathBuf::from("/data/reads.bai"))
        );
        let url = Location::parse("https://example.org/x/reads.bam?k=1").unwrap();
        assert!(url.is_remote());
        assert_eq!(url.name(), "reads.bam");
        assert_eq!(
            url.with_suffix(".bai").to_string(),
            "https://example.org/x/reads.bam.bai?k=1"
        );
        assert_eq!(url.path(), None);
        assert!(Location::parse("https://user:pass@example.org/a").is_err());
    }

    #[test]
    fn local_locations_open_readers() {
        let scratch = Scratch::new("location");
        let path = scratch.file("a.txt", "hello world");
        let location = Location::File(path.clone());
        assert_eq!(location.size(), Ok(11));
        assert_eq!(location.path(), Some(path.as_path()));
        let mut reader = location.open().unwrap();
        reader.seek(SeekFrom::Start(6)).unwrap();
        let mut text = String::new();
        reader.read_to_string(&mut text).unwrap();
        assert_eq!(text, "world");
        assert!(location.with_suffix(".missing").open().is_err());
    }
}
