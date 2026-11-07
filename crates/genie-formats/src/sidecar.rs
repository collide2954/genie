use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

use crate::Error;

const SCAN_BUFFER: usize = 1 << 20;

pub trait Sidecar: Sized {
    const EXTENSION: &'static str;

    fn parse(file: File) -> Result<Self, Error>;

    fn save(&self, file: File) -> io::Result<()>;
}

pub struct Counting<'a, R> {
    inner: R,
    read: u64,
    total: u64,
    percent: u64,
    report: &'a mut dyn FnMut(f32),
}

impl<'a, R> Counting<'a, R> {
    pub fn new(inner: R, total: u64, report: &'a mut dyn FnMut(f32)) -> Self {
        Self {
            inner,
            read: 0,
            total,
            percent: 0,
            report,
        }
    }
}

impl<R: Read> Read for Counting<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.read += count as u64;
        let percent = (self.read * 100)
            .checked_div(self.total)
            .unwrap_or(100)
            .min(100);
        if percent > self.percent {
            self.percent = percent;
            (self.report)(percent as f32 / 100.0);
        }
        Ok(count)
    }
}

pub fn load<'a, T: Sidecar>(
    data: &Path,
    cache: Option<&Path>,
    progress: &'a mut dyn FnMut(f32),
    build: impl FnOnce(BufReader<Counting<'a, File>>) -> Result<T, Error>,
) -> Result<T, Error> {
    let mut sidecar = data.as_os_str().to_owned();
    sidecar.push(".");
    sidecar.push(T::EXTENSION);
    let sidecar = PathBuf::from(sidecar);
    if sidecar.is_file() {
        return T::parse(File::open(sidecar)?);
    }
    let cached = cache.and_then(|cache| {
        fingerprint(data)
            .ok()
            .map(|key| cache.join(format!("{key:016x}.{}", T::EXTENSION)))
    });
    if let Some(cached) = cached.as_deref().filter(|path| path.is_file()) {
        return T::parse(File::open(cached)?);
    }
    let file = File::open(data)?;
    let total = file.metadata()?.len();
    let reader = Counting::new(file, total, progress);
    let index = build(BufReader::with_capacity(SCAN_BUFFER, reader))?;
    if let Some(cached) = &cached {
        let _ = store(cached, &index);
    }
    Ok(index)
}

fn fingerprint(data: &Path) -> io::Result<u64> {
    let metadata = fs::metadata(data)?;
    let mut hasher = DefaultHasher::new();
    fs::canonicalize(data)?.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    metadata.modified()?.hash(&mut hasher);
    Ok(hasher.finish())
}

fn store<T: Sidecar>(path: &Path, index: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let partial = path.with_extension(format!("{}.partial", T::EXTENSION));
    index.save(File::create(&partial)?)?;
    fs::rename(partial, path)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::Counting;

    #[test]
    fn counting_reports_whole_percents() {
        let mut shares = Vec::new();
        let mut report = |share| shares.push(share);
        let mut reader = Counting::new(&[0_u8; 1_000][..], 1_000, &mut report);
        let mut buffer = [0; 7];
        while reader.read(&mut buffer).unwrap() > 0 {}
        assert!(shares.len() > 90, "{}", shares.len());
        assert!(shares.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(shares.last(), Some(&1.0));
    }
}
