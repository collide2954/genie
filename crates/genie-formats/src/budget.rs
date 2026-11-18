//! A range cache, a request log and a byte cap for work with real data from the web, such as the
//! clips of the website. Only builds with the `net-budget` feature have them, and the tests of
//! genie turn the feature on. Variables of the environment turn on each part:
//!
//! - `GENIE_NET_CACHE`: a folder. Each fetched range goes there, and a later request for the same
//!   bytes, or for a part of them, reads them from there. The folder also keeps the total of the
//!   bytes that all runs fetched.
//! - `GENIE_NET_CAP`: the most bytes that all runs with the same folder can fetch. A request that
//!   would fetch more fails.
//! - `GENIE_NET_LOG`: a file that gets a line for each answer and each refused request.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use crate::http::{Fetched, Url};
use crate::{Error, md5};

/// The bytes that this run fetched from the network.
static FETCHED: AtomicU64 = AtomicU64::new(0);
/// Keeps two threads from changing the total or the index of a folder at the same time.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default)]
pub struct Budget {
    pub cache: Option<PathBuf>,
    pub cap: Option<u64>,
    pub log: Option<PathBuf>,
}

impl Budget {
    /// The budget that the variables of the environment set.
    pub fn from_env() -> Self {
        Self {
            cache: std::env::var_os("GENIE_NET_CACHE").map(PathBuf::from),
            cap: std::env::var("GENIE_NET_CAP")
                .ok()
                .and_then(|text| text.trim().parse().ok()),
            log: std::env::var_os("GENIE_NET_LOG").map(PathBuf::from),
        }
    }

    /// The bytes that all runs with the cache folder fetched from the network.
    pub fn total(&self) -> u64 {
        self.cache
            .as_ref()
            .and_then(|folder| fs::read_to_string(folder.join("total.txt")).ok())
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0)
    }

    /// A stored range that holds the bytes from `start` to `end`. The end of the file can end the
    /// range first.
    pub fn stored(&self, url: &Url, start: u64, end: u64) -> Option<Fetched> {
        let folder = self.cache.as_ref()?;
        let index = fs::read_to_string(folder.join("index.txt")).ok()?;
        let address = url.shown();
        index.lines().find_map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next()?;
            let first = fields.next()?.parse::<u64>().ok()?;
            let last = fields.next()?.parse::<u64>().ok()?;
            let size = fields.next()?.parse::<u64>().ok()?;
            let file = fields.next()?;
            let wanted = end.min(size).max(start);
            if name != address || first > start || wanted > last {
                return None;
            }
            let bytes = fs::read(folder.join(file)).ok()?;
            let from = usize::try_from(start - first).ok()?;
            let to = usize::try_from(wanted - first).ok()?;
            Some(Fetched {
                bytes: bytes.get(from..to)?.to_vec(),
                size,
            })
        })
    }

    /// Keeps a fetched range in the cache folder.
    pub fn store(&self, url: &Url, start: u64, fetched: &Fetched) {
        let Some(folder) = &self.cache else {
            return;
        };
        let last = start + fetched.bytes.len() as u64;
        let address = url.shown();
        let key = format!("{address} {start} {last}");
        let file = format!("{}.part", md5::hex(&md5::digest(key.as_bytes())));
        if fs::write(folder.join(&file), &fetched.bytes).is_err() {
            return;
        }
        let _guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let line = format!("{address}\t{start}\t{last}\t{}\t{file}", fetched.size);
        append(&folder.join("index.txt"), &line);
    }

    /// Fails when an answer of `length` bytes would take the bytes of all runs over the cap.
    pub fn room(&self, url: &Url, length: u64) -> Result<(), Error> {
        let Some(cap) = self.cap else {
            return Ok(());
        };
        let used = self.total();
        if used.saturating_add(length) <= cap {
            return Ok(());
        }
        let words = format!("{used} + {length} bytes is more than the cap of {cap} bytes");
        let address = url.shown();
        self.note(&format!("refused {address}: {words}"));
        Err(Error::Remote(format!(
            "genie did not fetch {address}: {words} (GENIE_NET_CAP)."
        )))
    }

    /// Adds the bytes of an answer to the total.
    pub fn count(&self, url: &Url, status: u16, bytes: usize) {
        let bytes = bytes as u64;
        FETCHED.fetch_add(bytes, Ordering::Relaxed);
        let all = {
            let _guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
            let all = self.total() + bytes;
            if let Some(folder) = &self.cache {
                let _ = fs::write(folder.join("total.txt"), all.to_string());
            }
            all
        };
        self.note(&format!(
            "{status} {} {bytes} bytes, {all} in all",
            url.shown()
        ));
    }

    fn note(&self, line: &str) {
        if let Some(path) = &self.log {
            append(path, line);
        }
    }
}

fn append(path: &Path, line: &str) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

/// The budget of this run, from the variables of the environment.
pub fn budget() -> &'static Budget {
    static BUDGET: OnceLock<Budget> = OnceLock::new();
    BUDGET.get_or_init(Budget::from_env)
}

/// The bytes that this run fetched from the network.
pub fn fetched() -> u64 {
    FETCHED.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::Budget;
    use crate::http::{Fetched, Url};

    fn folder(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("genie-budget-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn fetched(bytes: Vec<u8>, size: u64) -> Fetched {
        Fetched { bytes, size }
    }

    #[test]
    fn stored_ranges_give_the_same_bytes_and_the_bytes_inside_them() {
        let folder = folder("stored");
        let budget = Budget {
            cache: Some(folder.clone()),
            ..Budget::default()
        };
        let url = Url::parse("https://example.org/a.bam").unwrap();
        assert_eq!(budget.stored(&url, 100, 150), None);
        let part: Vec<u8> = (0..50).collect();
        budget.store(&url, 100, &fetched(part.clone(), 1_000));
        assert_eq!(budget.stored(&url, 100, 150), Some(fetched(part, 1_000)));
        let inside: Vec<u8> = (10..20).collect();
        assert_eq!(budget.stored(&url, 110, 120), Some(fetched(inside, 1_000)));
        assert_eq!(budget.stored(&url, 90, 120), None);
        assert_eq!(budget.stored(&url, 140, 160), None);
        let other = Url::parse("https://example.org/b.bam").unwrap();
        assert_eq!(budget.stored(&other, 100, 150), None);
        budget.store(&url, 950, &fetched(vec![7; 50], 1_000));
        assert_eq!(
            budget.stored(&url, 960, 2_000),
            Some(fetched(vec![7; 40], 1_000))
        );
        assert_eq!(Budget::default().stored(&url, 100, 150), None);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn the_cap_counts_the_bytes_of_all_runs() {
        let folder = folder("cap");
        let log = folder.join("fetch.log");
        let budget = Budget {
            cache: Some(folder.clone()),
            cap: Some(100),
            log: Some(log.clone()),
        };
        let url = Url::parse("https://example.org/a.bam").unwrap();
        budget.count(&url, 206, 60);
        assert_eq!(budget.total(), 60);
        assert!(budget.room(&url, 40).is_ok());
        assert!(budget.room(&url, 41).is_err());
        let later = Budget {
            cache: Some(folder.clone()),
            cap: Some(100),
            log: None,
        };
        assert_eq!(later.total(), 60);
        later.count(&url, 206, 30);
        assert_eq!(budget.total(), 90);
        let lines = std::fs::read_to_string(&log).unwrap();
        assert_eq!(lines.lines().count(), 2, "{lines}");
        assert!(lines.contains("206 https://example.org/a.bam 60 bytes"));
        assert!(lines.contains("refused https://example.org/a.bam"));
        assert!(Budget::default().room(&url, u64::MAX).is_ok());
        std::fs::remove_dir_all(folder).unwrap();
    }
}
