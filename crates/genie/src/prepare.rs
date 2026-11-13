use std::cell::RefCell;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, ErrorKind, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use genie_formats::http::{self, Url};
use genie_formats::sort::{self, Budget, Stage};
use genie_formats::tabix::{self, Layout};
use genie_formats::{Error, Reason, bai, bgzf, features, gzip};

use crate::location::Location;
use crate::track::{self, Prepared};

pub const STOPPED: &str = "Stopped.";
const ROOT: &str = "prepared";
const MARKER: &str = "prepared.txt";
const MEMORY: usize = 256 << 20;
const DOWNLOAD_LIMIT: u64 = 1 << 30;
const DOWNLOADED: &str = "downloaded";
pub const NO_CACHE: &str = "genie cannot find a cache folder for the indexes it makes.";
const HEAD: usize = 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Reads,
    Text(Layout),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Downloading,
    Indexing,
    Compressing,
    Sorting,
}

impl Step {
    pub fn verb(self) -> &'static str {
        match self {
            Self::Downloading => "Downloading",
            Self::Indexing => "Indexing",
            Self::Compressing => "Compressing",
            Self::Sorting => "Sorting",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    pub location: Location,
    pub name: String,
    kind: Kind,
    key: String,
    size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Ready,
    Prepared(Prepared),
    Needs(Work),
}

pub fn check(location: &Location, cache: Option<&Path>) -> Result<Check, String> {
    check_with(location, cache, track::PLAIN_LIMIT)
}

pub fn check_with(location: &Location, cache: Option<&Path>, limit: u64) -> Result<Check, String> {
    let name = location.name();
    let Some(kind) = kind(&name) else {
        return Ok(Check::Ready);
    };
    let opens = |suffixes: &[&str]| {
        suffixes
            .iter()
            .any(|suffix| location.with_suffix(suffix).open().is_ok())
    };
    let ready = match kind {
        Kind::Reads => opens(&[".bai", ".csi"]) || location.with_extension("bai").open().is_ok(),
        Kind::Text(_) if track::is_compressed(&name) => opens(&[".tbi", ".csi"]),
        Kind::Text(_) => location.size()? <= limit,
    };
    if ready {
        return Ok(Check::Ready);
    }
    let cache = cache.ok_or_else(|| NO_CACHE.to_owned())?;
    let size = location.size()?;
    let key = key(location, size);
    if let Some(prepared) = done(&cache.join(ROOT).join(&key), location) {
        return Ok(Check::Prepared(prepared));
    }
    if location.is_remote() && size > DOWNLOAD_LIMIT {
        return Err(format!(
            "The file {name} has no index, and it is larger than 1 GB. genie downloads files without an index only up to 1 GB."
        ));
    }
    Ok(Check::Needs(Work {
        location: location.clone(),
        name,
        kind,
        key,
        size,
    }))
}

fn kind(name: &str) -> Option<Kind> {
    if track::is_bam(name) {
        return Some(Kind::Reads);
    }
    if track::is_vcf(name) {
        return Some(Kind::Text(Layout::VCF));
    }
    if track::is_bigbed(name) || track::is_bigwig(name) {
        return None;
    }
    if track::is_bedgraph(name) {
        return Some(Kind::Text(Layout::BED));
    }
    Some(Kind::Text(match features::Kind::of(name)? {
        features::Kind::Bed => Layout::BED,
        features::Kind::Gff | features::Kind::Gtf => Layout::GFF,
    }))
}

fn key(location: &Location, size: u64) -> String {
    let mut parts = vec![
        location.to_string().into_bytes(),
        size.to_le_bytes().to_vec(),
    ];
    if let Location::File(path) = location {
        if let Ok(canonical) = fs::canonicalize(path) {
            parts[0] = canonical.to_string_lossy().into_owned().into_bytes();
        }
        let modified = fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok());
        if let Some(modified) = modified {
            parts.push(modified.as_nanos().to_le_bytes().to_vec());
        }
    }
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for part in &parts {
        for byte in part.iter().chain([&0xFF]) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    format!("{hash:016x}")
}

fn done(folder: &Path, location: &Location) -> Option<Prepared> {
    let text = fs::read_to_string(folder.join(MARKER)).ok()?;
    let value = |wanted: &str| {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once(" = ")?;
            (key == wanted).then(|| value.trim().to_owned())
        })
    };
    let index = folder.join(value("index")?);
    if !index.is_file() {
        return None;
    }
    let data = match value("data")?.as_str() {
        "" => location.clone(),
        name => {
            let path = folder.join(name);
            if !path.is_file() {
                return None;
            }
            Location::File(path)
        }
    };
    Some(Prepared {
        data,
        index: Location::File(index),
    })
}

pub fn run(
    work: &Work,
    cache: &Path,
    report: &mut dyn FnMut(Step, f32),
    stop: &AtomicBool,
) -> Result<Prepared, String> {
    let root = cache.join(ROOT);
    let folder = root.join(&work.key);
    let partial = root.join(format!("{}.partial", work.key));
    let _ = fs::remove_dir_all(&partial);
    fs::create_dir_all(&partial).map_err(|error| failed(&work.name, &error.into()))?;
    let progress = RefCell::new(Progress { report, last: None });
    let result = make(work, &partial, &progress, stop)
        .and_then(|(data, index)| {
            fs::write(
                partial.join(MARKER),
                format!("data = {}\nindex = {index}\n", data.unwrap_or_default()),
            )?;
            let _ = fs::remove_dir_all(&folder);
            fs::rename(&partial, &folder)?;
            Ok(())
        })
        .map_err(|error| failed(&work.name, &error));
    if result.is_err() {
        let _ = fs::remove_dir_all(&partial);
    }
    result?;
    done(&folder, &work.location).ok_or_else(|| format!("genie cannot prepare {}.", work.name))
}

type Made = (Option<String>, String);

fn make(
    work: &Work,
    partial: &Path,
    progress: &RefCell<Progress<'_>>,
    stop: &AtomicBool,
) -> Result<Made, Error> {
    let (source, downloaded) = match &work.location {
        Location::File(path) => (path.clone(), false),
        Location::Url(url) => {
            let part = partial.with_extension("download");
            fetch(url, &part, work.size, progress, stop)?;
            let target = partial.join(DOWNLOADED);
            fs::rename(&part, &target)?;
            (target, true)
        }
    };
    let job = Job {
        source: &source,
        partial,
        name: &work.name,
        total: fs::metadata(&source)?.len().max(1),
        progress,
        stop,
    };
    let (data, index) = match work.kind {
        Kind::Reads => job.reads(),
        Kind::Text(layout) => job.text(layout),
    }?;
    if !downloaded {
        return Ok((data, index));
    }
    match data {
        None => Ok((Some(DOWNLOADED.to_owned()), index)),
        Some(copy) => {
            fs::remove_file(&source)?;
            Ok((Some(copy), index))
        }
    }
}

fn fetch(
    url: &Url,
    part: &Path,
    size: u64,
    progress: &RefCell<Progress<'_>>,
    stop: &AtomicBool,
) -> Result<(), Error> {
    let mut file = OpenOptions::new().create(true).append(true).open(part)?;
    let from = file.metadata()?.len();
    let share = |done: u64, total: u64| done as f32 / total.max(1) as f32;
    progress
        .borrow_mut()
        .send(Step::Downloading, share(from, size));
    http::download(
        url,
        from,
        |bytes| {
            if stop.load(Ordering::Relaxed) {
                return Err(io::Error::other(Halted));
            }
            file.write_all(bytes)
        },
        |done, total| {
            progress
                .borrow_mut()
                .send(Step::Downloading, share(done, total));
        },
    )?;
    file.sync_all()?;
    Ok(())
}

struct Progress<'a> {
    report: &'a mut dyn FnMut(Step, f32),
    last: Option<(Step, u32)>,
}

impl Progress<'_> {
    fn send(&mut self, step: Step, share: f32) {
        let percent = (share.clamp(0.0, 1.0) * 100.0) as u32;
        if self.last != Some((step, percent)) {
            self.last = Some((step, percent));
            (self.report)(step, share.clamp(0.0, 1.0));
        }
    }
}

struct Job<'a, 'p> {
    source: &'a Path,
    partial: &'a Path,
    name: &'a str,
    total: u64,
    progress: &'a RefCell<Progress<'p>>,
    stop: &'a AtomicBool,
}

impl<'a, 'p> Job<'a, 'p> {
    fn reads(&self) -> Result<Made, Error> {
        let index = format!("{}.bai", self.name);
        match bai::build(self.watched(Step::Indexing, 1.0)?) {
            Ok(bytes) => {
                fs::write(self.partial.join(&index), bytes)?;
                return Ok((None, index));
            }
            Err(Error::Invalid(Reason::Unsorted | Reason::Scattered)) => {}
            Err(error) => return Err(error),
        }
        let copy = self.name.to_owned();
        let output = BufWriter::new(File::create(self.partial.join(&copy))?);
        let budget = Budget {
            folder: self.partial,
            memory: MEMORY,
        };
        let bytes = sort::bam(
            self.watched(Step::Sorting, 0.5)?,
            output,
            &budget,
            &mut |stage| self.staged(stage),
        )?;
        fs::write(self.partial.join(&index), bytes)?;
        Ok((Some(copy), index))
    }

    fn text(&self, layout: Layout) -> Result<Made, Error> {
        let compressed = track::is_compressed(self.name);
        let mut head = Vec::with_capacity(HEAD);
        File::open(self.source)?
            .take(HEAD as u64)
            .read_to_end(&mut head)?;
        let copy = if compressed {
            self.name.to_owned()
        } else {
            format!("{}.gz", self.name)
        };
        for csi in [false, true] {
            let attempt = if compressed && bgzf::is_bgzf(&head) {
                tabix::build(self.watched(Step::Indexing, 1.0)?, layout, csi)
                    .map(|bytes| (None, bytes))
            } else {
                let output = BufWriter::new(File::create(self.partial.join(&copy))?);
                let input = self.lines(Step::Compressing, 1.0, compressed)?;
                sort::copy_text(input, layout, csi, output, &mut |_| !self.stopped())
                    .map(|bytes| (Some(copy.clone()), bytes))
            };
            match attempt {
                Ok((data, bytes)) => {
                    let index = format!("{}.tbi", data.as_deref().unwrap_or(self.name));
                    fs::write(self.partial.join(&index), bytes)?;
                    return Ok((data, index));
                }
                Err(Error::Format {
                    reason: Reason::TooLong,
                    ..
                }) if !csi => {}
                Err(Error::Format {
                    reason: Reason::Unsorted | Reason::Scattered,
                    ..
                }) => break,
                Err(error) => return Err(error),
            }
        }
        let output = BufWriter::new(File::create(self.partial.join(&copy))?);
        let budget = Budget {
            folder: self.partial,
            memory: MEMORY,
        };
        let input = self.lines(Step::Sorting, 0.5, compressed)?;
        let bytes = sort::text(input, layout, output, &budget, &mut |stage| {
            self.staged(stage)
        })?;
        let index = format!("{copy}.tbi");
        fs::write(self.partial.join(&index), bytes)?;
        Ok((Some(copy), index))
    }

    fn lines(&self, step: Step, scale: f32, compressed: bool) -> io::Result<Box<dyn BufRead + '_>> {
        let watched = BufReader::new(self.watched(step, scale)?);
        Ok(if compressed {
            Box::new(BufReader::new(gzip::Reader::new(watched)))
        } else {
            Box::new(watched)
        })
    }

    fn watched(&self, step: Step, scale: f32) -> io::Result<Watched<'_, 'a, 'p>> {
        self.progress.borrow_mut().send(step, 0.0);
        Ok(Watched {
            inner: File::open(self.source)?,
            read: 0,
            job: self,
            step,
            scale,
        })
    }

    fn staged(&self, stage: Stage) -> bool {
        if let Stage::Writing { done, total } = stage {
            let share = done as f32 / total.max(1) as f32;
            self.progress
                .borrow_mut()
                .send(Step::Sorting, 0.5 + 0.5 * share);
        }
        !self.stopped()
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

struct Watched<'j, 'a, 'p> {
    inner: File,
    read: u64,
    job: &'j Job<'a, 'p>,
    step: Step,
    scale: f32,
}

impl Read for Watched<'_, '_, '_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.job.stopped() {
            return Err(io::Error::other(Halted));
        }
        let count = self.inner.read(buffer)?;
        self.read += count as u64;
        let share = self.read as f32 / self.job.total as f32;
        self.job
            .progress
            .borrow_mut()
            .send(self.step, share * self.scale);
        Ok(count)
    }
}

#[derive(Debug)]
struct Halted;

impl fmt::Display for Halted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(STOPPED)
    }
}

impl std::error::Error for Halted {}

fn failed(name: &str, error: &Error) -> String {
    let halted = match error {
        Error::Io(error) => {
            error.kind() == ErrorKind::Interrupted
                || matches!(error.get_ref(), Some(inner) if inner.is::<Halted>())
        }
        _ => false,
    };
    if halted {
        STOPPED.to_owned()
    } else {
        format!("genie cannot prepare {name}. {error}")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;

    use genie_core::Region;

    use super::{Check, STOPPED, Step, check_with, run};
    use crate::location::Location;
    use crate::scratch::Scratch;
    use crate::track::{self, Content, Options};

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../genie-formats/tests/data")
            .join(name)
    }

    fn folder(scratch: &Scratch) -> PathBuf {
        let marker = scratch.file("marker", "");
        marker.parent().unwrap().to_owned()
    }

    fn copy(scratch: &Scratch, name: &str) -> Location {
        scratch.file(name, fs::read(fixture(name)).unwrap()).into()
    }

    fn prepare(location: &Location, cache: &Path) -> (track::Prepared, Vec<Step>) {
        let Check::Needs(work) = check_with(location, Some(cache), 0).unwrap() else {
            panic!("expected work for {location}");
        };
        let mut steps = Vec::new();
        let stop = AtomicBool::new(false);
        let prepared = run(&work, cache, &mut |step, _| steps.push(step), &stop).unwrap();
        (prepared, steps)
    }

    fn feature_names(source: &track::Source, contig: &str) -> Vec<String> {
        let region = Region::new(0, 1_000_000).unwrap();
        let options = Options::default();
        let loaded = track::load(source, contig, region, options, 1, true, |_| {
            Err(String::new())
        });
        let Content::Features(annotation) = &loaded.unwrap().content else {
            panic!("expected features");
        };
        annotation
            .features
            .iter()
            .map(|feature| feature.name().to_owned())
            .collect()
    }

    #[test]
    fn indexed_files_and_small_plain_files_are_ready() {
        let scratch = Scratch::new("prepare-ready");
        let cache = folder(&scratch).join("cache");
        let ready = |location: Location, limit: u64| {
            check_with(&location, Some(&cache), limit).unwrap() == Check::Ready
        };
        assert!(ready(fixture("genes.bed.gz").into(), 0));
        assert!(ready(fixture("reads.bam").into(), 0));
        assert!(ready(fixture("signal.bw").into(), 0));
        assert!(ready(
            scratch.file("small.bed", "chr1\t10\t20\n").into(),
            1_000
        ));
        assert!(!ready(
            scratch.file("large.bed", "chr1\t10\t20\n").into(),
            5
        ));
    }

    #[test]
    fn a_bam_without_an_index_gets_one_in_the_cache() {
        let scratch = Scratch::new("prepare-bam");
        let cache = folder(&scratch).join("cache");
        let reads = copy(&scratch, "reads.bam");
        let (prepared, steps) = prepare(&reads, &cache);
        assert_eq!(prepared.data, reads);
        assert!(steps.contains(&Step::Indexing), "{steps:?}");
        assert_eq!(
            check_with(&reads, Some(&cache), 0).unwrap(),
            Check::Prepared(prepared.clone())
        );
        assert!(track::open_with(reads, Some(prepared)).unwrap().has_reads());
    }

    #[test]
    fn unsorted_text_is_sorted_into_the_cache() {
        let scratch = Scratch::new("prepare-sort");
        let cache = folder(&scratch).join("cache");
        let text =
            "#chrom\tstart\tend\tname\nchr1\t300\t400\tc\nchr2\t5\t9\td\nchr1\t100\t200\ta\n";
        let file: Location = scratch.file("genes.bed", text).into();
        let (prepared, steps) = prepare(&file, &cache);
        assert_ne!(prepared.data, file);
        assert_eq!(steps.first(), Some(&Step::Compressing));
        assert!(steps.contains(&Step::Sorting), "{steps:?}");
        let source = track::open_with(file, Some(prepared)).unwrap();
        assert_eq!(source.name, "genes.bed");
        assert_eq!(feature_names(&source, "chr1"), ["a", "c"]);
    }

    #[test]
    fn plain_gzip_is_copied_as_bgzip_and_bgzip_only_gets_an_index() {
        let scratch = Scratch::new("prepare-gzip");
        let cache = folder(&scratch).join("cache");
        let text = "chr1\t100\t200\ta\nchr1\t300\t400\tb\n";
        let gzip: Location = scratch
            .file(
                "genes.bed.gz",
                genie_formats::gzip::compress(text.as_bytes()),
            )
            .into();
        let (prepared, steps) = prepare(&gzip, &cache);
        assert_ne!(prepared.data, gzip);
        assert!(!steps.contains(&Step::Sorting), "{steps:?}");
        let source = track::open_with(gzip, Some(prepared)).unwrap();
        assert_eq!(feature_names(&source, "chr1"), ["a", "b"]);
        let variants = copy(&scratch, "variants.vcf.gz");
        let (prepared, steps) = prepare(&variants, &cache);
        assert_eq!(prepared.data, variants);
        assert_eq!(steps.first(), Some(&Step::Indexing));
        assert!(
            track::open_with(variants, Some(prepared))
                .unwrap()
                .has_variants()
        );
    }

    #[test]
    fn web_files_without_an_index_are_downloaded_then_indexed() {
        let scratch = Scratch::new("prepare-web");
        let cache = folder(&scratch).join("cache");
        let bytes = fs::read(fixture("reads.bam")).unwrap();
        let server = crate::server::serve(vec![("/reads.bam".to_owned(), bytes)]);
        let location = Location::parse(&server.url("/reads.bam")).unwrap();
        let (prepared, mut steps) = prepare(&location, &cache);
        steps.dedup();
        assert_eq!(steps, [Step::Downloading, Step::Indexing]);
        assert!(!prepared.data.is_remote());
        assert_eq!(
            check_with(&location, Some(&cache), 0).unwrap(),
            Check::Prepared(prepared.clone())
        );
        let source = track::open_with(location, Some(prepared)).unwrap();
        assert!(source.has_reads());
        assert!(!source.data().is_remote());
    }

    #[test]
    fn stopped_downloads_go_on_from_where_they_stopped() {
        let scratch = Scratch::new("prepare-resume");
        let cache = folder(&scratch).join("cache");
        let bytes = fs::read(fixture("reads.bam")).unwrap();
        let half = bytes.len() / 2;
        let server = crate::server::serve(vec![("/reads.bam".to_owned(), bytes.clone())]);
        let location = Location::parse(&server.url("/reads.bam")).unwrap();
        let Check::Needs(work) = check_with(&location, Some(&cache), 0).unwrap() else {
            panic!("expected work");
        };
        let root = cache.join("prepared");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(format!("{}.download", work.key)), &bytes[..half]).unwrap();
        let mut shares = Vec::new();
        let stop = AtomicBool::new(false);
        let mut report = |step, share| {
            if step == Step::Downloading {
                shares.push(share);
            }
        };
        let prepared = run(&work, &cache, &mut report, &stop).unwrap();
        assert!(shares[0] >= 0.49, "{shares:?}");
        let Location::File(data) = &prepared.data else {
            panic!("expected a local copy");
        };
        assert_eq!(fs::read(data).unwrap(), bytes);
    }

    #[test]
    fn a_stop_ends_the_work_and_leaves_no_files() {
        let scratch = Scratch::new("prepare-stop");
        let cache = folder(&scratch).join("cache");
        let reads = copy(&scratch, "reads.bam");
        let Check::Needs(work) = check_with(&reads, Some(&cache), 0).unwrap() else {
            panic!("expected work");
        };
        let stop = AtomicBool::new(true);
        let result = run(&work, &cache, &mut |_, _| {}, &stop);
        assert_eq!(result, Err(STOPPED.to_owned()));
        let left: Vec<_> = fs::read_dir(cache.join("prepared")).unwrap().collect();
        assert!(left.is_empty(), "{left:?}");
        assert!(matches!(
            check_with(&reads, Some(&cache), 0).unwrap(),
            Check::Needs(_)
        ));
    }
}
