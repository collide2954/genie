use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use crate::Error;

const SCAN_BUFFER: usize = 1 << 20;

pub trait Sidecar: Sized {
    const EXTENSION: &'static str;

    fn parse(file: File) -> Result<Self, Error>;

    fn save(&self, file: File) -> io::Result<()>;
}

pub fn load<T: Sidecar>(
    data: &Path,
    cache: Option<&Path>,
    build: impl FnOnce(BufReader<File>) -> Result<T, Error>,
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
    let index = build(BufReader::with_capacity(SCAN_BUFFER, File::open(data)?))?;
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
