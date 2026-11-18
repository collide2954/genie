//! The broken-file sweep: cut and damaged copies of each test file go to the readers of genie.
//! A reader must give an error or data. It must not panic, and it must not hang.

use std::any::Any;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use genie_core::{Assembly, Contig, Region};
use genie_formats::chain::Chains;
use genie_formats::gzip;

use crate::reference::{self, Reference};
use crate::track::{self, Options};

/// The longest time that the readers may take for one damaged file.
const LIMIT: Duration = Duration::from_secs(30);
/// The region that the readers load on each contig.
const REGION: Region = Region::between(0, 3_000_000);

/// The test files to damage, each with the files to copy next to it as they are.
const FIXTURES: &[(&str, &[&str])] = &[
    ("calls.bcf", &["calls.bcf.csi"]),
    ("calls.bcf.csi", &["calls.bcf"]),
    ("ccres.bb", &[]),
    ("gencode.pred.bb", &[]),
    ("genes.bb", &[]),
    ("genes.pred.bb", &[]),
    ("interact.bb", &[]),
    ("snps.bb", &[]),
    ("contacts.hic", &[]),
    ("contacts.mcool", &[]),
    ("cram.bam", &["cram.bam.bai"]),
    ("cram.bam.bai", &["cram.bam"]),
    ("cram30.cram", &["cram30.cram.crai"]),
    ("cram30.cram.crai", &["cram30.cram"]),
    ("cram31.cram", &["cram31.cram.crai"]),
    ("cram-codecs.cram", &["cram-codecs.cram.crai"]),
    ("cram-embed.cram", &["cram-embed.cram.crai"]),
    ("cram-multi.cram", &["cram-multi.cram.crai"]),
    ("cram-noref.cram", &["cram-noref.cram.crai"]),
    ("cram-xz.cram", &["cram-xz.cram.crai"]),
    ("genes.bed.gz", &["genes.bed.gz.tbi"]),
    ("genes.bed.gz.tbi", &["genes.bed.gz"]),
    ("genes.gff3.gz", &["genes.gff3.gz.tbi"]),
    ("genes.gtf.gz", &["genes.gtf.gz.tbi"]),
    ("genes.gtf.gz.tbi", &["genes.gtf.gz"]),
    ("mods.bam", &["mods.bam.bai"]),
    ("pairs.bam", &["pairs.bam.bai"]),
    ("reads.bam", &["reads.bam.bai"]),
    ("reads.bam.bai", &["reads.bam"]),
    ("plain.bedGraph.gz", &["plain.bedGraph.gz.tbi"]),
    ("plain.bedGraph.gz.tbi", &["plain.bedGraph.gz"]),
    ("plain.bw", &[]),
    ("signal.bw", &[]),
    ("steps.bw", &[]),
    (
        "reference.fa.gz",
        &["reference.fa.gz.fai", "reference.fa.gz.gzi"],
    ),
    (
        "reference.fa.gz.fai",
        &["reference.fa.gz", "reference.fa.gz.gzi"],
    ),
    (
        "reference.fa.gz.gzi",
        &["reference.fa.gz", "reference.fa.gz.fai"],
    ),
    ("variants.vcf.gz", &["variants.vcf.gz.tbi"]),
    ("variants.vcf.gz.tbi", &["variants.vcf.gz"]),
    ("variants.vcf.gz.csi", &["variants.vcf.gz"]),
];

/// Compressed test files that genie also reads as plain text, with the name of the plain copy.
const PLAIN: &[(&str, &str)] = &[
    ("genes.bed.gz", "genes.bed"),
    ("genes.gff3.gz", "genes.gff3"),
    ("genes.gtf.gz", "genes.gtf"),
    ("plain.bedGraph.gz", "plain.bedGraph"),
    ("variants.vcf.gz", "variants.vcf"),
    ("reference.fa.gz", "reference.fa"),
];

/// Small text files of the formats that have no test file of their own.
const TEXTS: &[(&str, &str)] = &[
    (
        "lift.chain",
        "chain 1000 chr1 5000 + 0 3000 chr1 6000 + 0 3100 1\n1000 0 100\n2000\n\n\
         chain 500 chr2 1000 + 100 600 chrB 2000 - 200 700 2\n500\n",
    ),
    (
        "study.tsv",
        "chromosome\tbase_pair_location\teffect_allele\tother_allele\tbeta\tp_value\trsid\n\
         1\t1500\tA\tG\t0.12\t3e-9\trs1\n1\t2500\tC\tT\t-0.05\t0.002\trs2\n\
         2\t800\tG\tA\t0.3\t1e-12\trs3\n",
    ),
    (
        "tumor.seg",
        "ID\tchrom\tloc.start\tloc.end\tnum.mark\tseg.mean\n\
         T1\t1\t100\t5000\t40\t0.85\nT1\t1\t5000\t9000\t20\t-1.2\nT2\t2\t50\t800\t9\t0.1\n",
    ),
    (
        "loops.bedpe",
        "#chrom1\tstart1\tend1\tchrom2\tstart2\tend2\tname\tscore\n\
         chr1\t100\t200\tchr1\t5000\t5100\tloop1\t5\nchr1\t300\t400\tchr2\t900\t1000\tloop2\t2\n",
    ),
    (
        "sites.bedMethyl",
        "chr1\t100\t101\tm\t20\t+\t100\t101\t255,0,0\t20\t75.00\t15\t5\t0\t0\t0\t0\t0\n\
         chr1\t101\t102\tm\t18\t-\t101\t102\t255,0,0\t18\t50.00\t9\t9\t0\t0\t0\t0\t0\n",
    ),
    (
        "liver.SJ.out.tab",
        "chr1\t1001\t2000\t1\t1\t1\t12\t3\t40\nchr1\t5001\t9000\t2\t2\t0\t4\t0\t25\n\
         chr2\t101\t300\t0\t0\t0\t1\t2\t18\n",
    ),
    (
        "liver_junctions.bed",
        "track name=junctions description=\"TopHat junctions\"\n\
         chr1\t950\t2050\tJUNC1\t12\t+\t950\t2050\t255,0,0\t2\t50,50\t0,1050\n\
         chr2\t60\t340\tJUNC2\t3\t-\t60\t340\t0,0,255\t2\t40,40\t0,240\n",
    ),
    (
        "hits.psl",
        "psLayout version 3\n\n\
         95\t3\t2\t0\t1\t10\t1\t200\t-\tread7\t150\t20\t130\tchr1\t5000\t1000\t1310\t2\t50,50,\t20,80,\t1000,1260,\n\
         24\t0\t0\t0\t0\t0\t0\t0\t+\tread8\t24\t0\t24\tchr2\t1000\t100\t124\t1\t24,\t0,\t100,\n",
    ),
    (
        "assembly.paf",
        "ctg1\t9000\t100\t2100\t-\tchr1\t5000\t100\t2100\t1900\t2000\t60\ttp:A:P\tcg:Z:900M100D1000M\n\
         ctg2\t800\t0\t800\t+\tchr2\t1000\t100\t900\t790\t800\t255\tNM:i:10\n",
    ),
];

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../genie-formats/tests/data")
}

/// The bytes of a file, each with its name, and the files to copy next to it.
struct Case {
    name: String,
    bytes: Vec<u8>,
    others: Vec<(String, Vec<u8>)>,
}

fn cases() -> Vec<Case> {
    let read = |name: &str| fs::read(data().join(name)).unwrap();
    let mut cases: Vec<Case> = FIXTURES
        .iter()
        .map(|(name, others)| Case {
            name: (*name).to_owned(),
            bytes: read(name),
            others: others
                .iter()
                .map(|other| ((*other).to_owned(), read(other)))
                .collect(),
        })
        .collect();
    for (compressed, name) in PLAIN {
        let mut bytes = Vec::new();
        gzip::Reader::new(BufReader::new(File::open(data().join(compressed)).unwrap()))
            .read_to_end(&mut bytes)
            .unwrap();
        cases.push(Case {
            name: (*name).to_owned(),
            bytes,
            others: Vec::new(),
        });
    }
    cases.extend(TEXTS.iter().map(|(name, text)| Case {
        name: (*name).to_owned(),
        bytes: text.as_bytes().to_vec(),
        others: Vec::new(),
    }));
    cases
}

/// A number from the name of a file, so that each file gets its own damage.
fn seed(name: &str) -> u64 {
    name.bytes().fold(0x9E37_79B9_7F4A_7C15, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3)
    })
}

/// Damaged copies of the bytes of a file, each with words that say what changed: cuts at the
/// start, in the middle and before the end, changed bytes, and runs of 0xFF and 0x00 bytes. A
/// quick sweep makes 3 copies of each file, and a full sweep about 24.
fn damages(bytes: &[u8], seed: u64, quick: bool) -> Vec<(String, Vec<u8>)> {
    let length = bytes.len();
    let cuts = if quick {
        vec![length / 2]
    } else {
        vec![0, 1, 4, 12, 28, 64, 200, length / 3, length / 2, length - 1]
    };
    let mut found = Vec::new();
    for cut in cuts {
        if cut < length {
            found.push((format!("cut to {cut} bytes"), bytes[..cut].to_vec()));
        }
    }
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..if quick { 1 } else { 8 } {
        let at = (next() % length as u64) as usize;
        let mut copy = bytes.to_vec();
        copy[at] ^= (next() % 255 + 1) as u8;
        found.push((format!("byte {at} changed"), copy));
    }
    let runs: &[(u8, usize)] = if quick {
        &[(0xFF, 1)]
    } else {
        &[(0xFF, 3), (0x00, 3)]
    };
    for &(fill, count) in runs {
        for _ in 0..count {
            let at = (next() % length as u64) as usize;
            let mut copy = bytes.to_vec();
            copy[at..(at + 16).min(length)].fill(fill);
            found.push((format!("16 bytes from {at} set to {fill:#04x}"), copy));
        }
    }
    found
}

/// Opens a file with each reader that can read it, and loads data from it.
fn exercise(path: &Path, reference: &Reference) {
    if let Ok(source) = track::open(path.to_path_buf()) {
        let contigs: Vec<String> = source.contigs().iter().take(3).cloned().collect();
        let assembly = Assembly::new(
            contigs
                .iter()
                .map(|name| Contig {
                    name: name.clone(),
                    length: 250_000_000,
                })
                .collect(),
        );
        for name in &contigs {
            let index = reference.assembly.find(name);
            let bases = |window: Region| match index {
                Some(index) => reference.fetch(index, window),
                None => Err("There is no such contig.".to_owned()),
            };
            for (step, detail) in [(1, true), (20_000, false)] {
                let options = Options::default();
                let _ = track::load(&source, name, REGION, options, step, detail, bases);
            }
        }
        let _ = track::names(&source, &assembly, None);
    }
    let name = path.to_string_lossy();
    if [".fa", ".fa.gz", ".fai", ".gzi"]
        .iter()
        .any(|end| name.ends_with(end))
    {
        let fasta = PathBuf::from(name.trim_end_matches(".fai").trim_end_matches(".gzi"));
        if let Ok(found) = reference::open(fasta, None, &mut |_| {}) {
            for contig in 0..found.assembly.contigs().len().min(3) {
                let _ = found.fetch(contig, Region::between(0, 20_000));
            }
        }
    }
    if name.ends_with(".chain")
        && let Ok(file) = File::open(path)
        && let Ok(chains) = Chains::read(BufReader::new(file))
    {
        let _ = chains.lift("chr1", Region::between(0, 4_000));
        let _ = chains.back("chr1", Region::between(0, 4_000));
    }
}

fn panic_words(error: &(dyn Any + Send)) -> String {
    error
        .downcast_ref::<&str>()
        .map(|words| (*words).to_owned())
        .or_else(|| error.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without words".to_owned())
}

/// Gives each damaged copy of each case to the readers, each in its own thread, and returns what
/// went wrong and the number of copies.
fn sweep(quick: bool) -> (Vec<String>, usize) {
    let reference = reference::open(data().join("reference.fa.gz"), None, &mut |_| {}).unwrap();
    let reference = Arc::new(reference);
    let kind = if quick { "quick" } else { "full" };
    let root = std::env::temp_dir().join(format!("genie-sweep-{kind}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let (mut failures, mut count) = (Vec::new(), 0);
    for case in cases() {
        let copies = damages(&case.bytes, seed(&case.name), quick);
        for (number, (damage, broken)) in copies.into_iter().enumerate() {
            count += 1;
            let folder = root.join(format!("{}-{number}", case.name));
            fs::create_dir_all(&folder).unwrap();
            for (other, bytes) in &case.others {
                fs::write(folder.join(other), bytes).unwrap();
            }
            let path = folder.join(&case.name);
            fs::write(&path, broken).unwrap();
            let (sender, receiver) = mpsc::channel();
            let shared = Arc::clone(&reference);
            thread::spawn(move || {
                let result = panic::catch_unwind(AssertUnwindSafe(|| exercise(&path, &shared)));
                let _ = sender.send(result.map_err(|error| panic_words(error.as_ref())));
            });
            let name = &case.name;
            match receiver.recv_timeout(LIMIT) {
                Ok(Ok(())) => {}
                Ok(Err(words)) => failures.push(format!("{name}, {damage}: panic: {words}")),
                Err(_) => failures.push(format!("{name}, {damage}: no end after {LIMIT:?}")),
            }
        }
    }
    let _ = fs::remove_dir_all(&root);
    (failures, count)
}

#[test]
fn damaged_files_give_errors_and_never_panic_or_hang() {
    let (failures, count) = sweep(true);
    assert!(
        failures.is_empty(),
        "{} of {count} damaged files failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "takes about two minutes: cargo test -p genie --bin genie sweep -- --ignored"]
fn every_damage_of_every_file_gives_an_error_and_never_a_panic_or_a_hang() {
    let (failures, count) = sweep(false);
    assert!(
        failures.is_empty(),
        "{} of {count} damaged files failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
