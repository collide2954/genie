use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use genie_core::{Band, Feature};
use genie_formats::bbi;
use genie_formats::http::{self, Url};
use genie_formats::{bed, contigs, cytoband, genepred, gzip};

use crate::location::Location;
use crate::paths;
use crate::reference::{self, Extras, Reference};

const GOLDEN: &str = "https://hgdownload.soe.ucsc.edu/goldenPath";
const T2T: &str = "https://hgdownload.soe.ucsc.edu/hubs/GCA/009/914/755/GCA_009914755.4/bbi/GCA_009914755.4_T2T-CHM13v2.0";
const FRESH: Duration = Duration::from_hours(30 * 24);
const GENOMES: &str = "genomes";
pub const SEQUENCE: &str = "sequence.2bit";
const PART: &str = "sequence.2bit.part";
pub const STOPPED: &str = "The download stopped.";
const SIZES: &str = "sizes.txt";
const ALIASES: &str = "aliases.txt";
const BANDS: &str = "bands.txt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub url: String,
    pub big: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Genome {
    pub id: String,
    pub label: String,
    pub species: String,
    pub assembly: String,
    pub sequence: String,
    pub sizes: String,
    pub aliases: String,
    pub bands: Option<Annotation>,
    pub genes: Annotation,
    pub track: String,
    pub megabytes: u64,
}

#[derive(Debug)]
pub struct Prepared {
    pub sequence: Location,
    pub extras: Extras,
}

impl Genome {
    pub fn folder(&self, cache: &Path) -> PathBuf {
        cache.join(GENOMES).join(&self.id)
    }

    pub fn genes_in(&self, folder: &Path) -> PathBuf {
        folder.join(format!("{}.bed", self.track))
    }
}

pub fn all() -> Vec<Genome> {
    vec![
        ucsc("hg38", "Human", "GRCh38", (true, true), 835),
        ucsc("hg19", "Human", "GRCh37", (true, true), 816),
        t2t(),
        ucsc("mm39", "Mouse", "GRCm39", (true, true), 714),
        ucsc("mm10", "Mouse", "GRCm38", (true, true), 715),
        ucsc("rn7", "Rat", "mRatBN7.2", (false, true), 692),
        ucsc("danRer11", "Zebrafish", "GRCz11", (false, true), 442),
        ucsc("dm6", "Fly", "BDGP6", (false, true), 37),
        ucsc("ce11", "Worm", "WBcel235", (false, true), 26),
        ucsc("sacCer3", "Yeast", "R64", (false, false), 3),
    ]
}

pub fn find(id: &str) -> Option<Genome> {
    all().into_iter().find(|genome| genome.id == id)
}

fn ucsc(
    id: &str,
    species: &str,
    assembly: &str,
    (select, bands): (bool, bool),
    megabytes: u64,
) -> Genome {
    let path = |file: &str| format!("{GOLDEN}/{id}/{file}");
    let table = if select {
        "ncbiRefSeqSelect"
    } else {
        "ncbiRefSeq"
    };
    Genome {
        id: id.to_owned(),
        label: id.to_owned(),
        species: species.to_owned(),
        assembly: assembly.to_owned(),
        sequence: path(&format!("bigZips/{id}.2bit")),
        sizes: path(&format!("bigZips/{id}.chrom.sizes")),
        aliases: path(&format!("bigZips/{id}.chromAlias.txt")),
        bands: bands.then(|| Annotation {
            url: path("database/cytoBandIdeo.txt.gz"),
            big: false,
        }),
        genes: Annotation {
            url: path(&format!("database/{table}.txt.gz")),
            big: false,
        },
        track: if select {
            "RefSeq Select"
        } else {
            "NCBI RefSeq"
        }
        .to_owned(),
        megabytes,
    }
}

fn t2t() -> Genome {
    let mut genome = ucsc("hs1", "Human", "T2T-CHM13v2.0", (false, false), 812);
    "T2T".clone_into(&mut genome.label);
    genome.bands = Some(Annotation {
        url: format!("{T2T}.cytoBandMapped/cytoBandMapped.bb"),
        big: true,
    });
    genome.genes = Annotation {
        url: format!("{T2T}.ncbiRefSeq.bb"),
        big: true,
    };
    genome
}

pub fn open(genome: &Genome, folder: &Path, now: SystemTime) -> Result<Reference, String> {
    let prepared = prepare(genome, folder, now)?;
    with_genome(genome, prepared.sequence, &prepared.extras)
}

pub fn online(genome: &Genome, folder: &Path) -> Result<Reference, String> {
    with_genome(genome, remote(genome)?, &extras(folder)?)
}

fn with_genome(genome: &Genome, sequence: Location, extras: &Extras) -> Result<Reference, String> {
    let mut reference = reference::open_with(sequence, extras, None, &mut |_| {})?;
    reference.genome = Some(genome.id.clone());
    Ok(reference)
}

fn remote(genome: &Genome) -> Result<Location, String> {
    Url::parse(&genome.sequence)
        .map(Location::Url)
        .map_err(|error| error.to_string())
}

pub fn offline(folder: &Path) -> bool {
    folder.join(SEQUENCE).exists()
}

pub fn download(
    genome: &Genome,
    folder: &Path,
    stop: &AtomicBool,
    progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let failed = |error: io::Error| error.to_string();
    fs::create_dir_all(folder).map_err(failed)?;
    let part = folder.join(PART);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&part)
        .map_err(failed)?;
    let from = file.metadata().map_err(failed)?.len();
    let url = Url::parse(&genome.sequence).map_err(|error| error.to_string())?;
    let result = http::download(
        &url,
        from,
        |bytes| {
            if stop.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, STOPPED));
            }
            file.write_all(bytes)
        },
        progress,
    );
    match result {
        Ok(_) => {}
        Err(_) if stop.load(Ordering::Relaxed) => return Err(STOPPED.to_owned()),
        Err(error) => return Err(error.to_string()),
    }
    file.sync_all().map_err(failed)?;
    drop(file);
    fs::rename(&part, folder.join(SEQUENCE)).map_err(failed)
}

pub fn remove(folder: &Path) -> io::Result<()> {
    for name in [SEQUENCE, PART] {
        match fs::remove_file(folder.join(name)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

pub fn prepare(genome: &Genome, folder: &Path, now: SystemTime) -> Result<Prepared, String> {
    fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    let what = |part: &str| format!("the {part} of {}", genome.label);
    keep(
        &genome.sizes,
        &folder.join(SIZES),
        now,
        &what("contig sizes"),
        text,
    )?;
    keep(
        &genome.aliases,
        &folder.join(ALIASES),
        now,
        &what("contig names"),
        text,
    )?;
    if let Some(bands) = &genome.bands {
        let big = bands.big;
        keep(
            &bands.url,
            &folder.join(BANDS),
            now,
            &what("bands"),
            |bytes| band_text(bytes, big),
        )?;
    }
    let genes = genome.genes_in(folder);
    let big = genome.genes.big;
    keep(&genome.genes.url, &genes, now, &what("genes"), |bytes| {
        gene_text(bytes, big)
    })?;
    let offline = folder.join(SEQUENCE);
    let sequence = if offline.exists() {
        Location::File(offline)
    } else {
        remote(genome)?
    };
    Ok(Prepared {
        sequence,
        extras: extras(folder)?,
    })
}

fn keep(
    url: &str,
    path: &Path,
    now: SystemTime,
    what: &str,
    convert: impl FnOnce(Vec<u8>) -> Result<String, String>,
) -> Result<(), String> {
    if fresh(path, now) {
        return Ok(());
    }
    let fetched = Url::parse(url)
        .and_then(|url| http::get(&url))
        .map_err(|error| error.to_string())
        .and_then(convert);
    match fetched {
        Ok(content) => paths::replace(path, &content).map_err(|error| error.to_string()),
        Err(_) if path.exists() => Ok(()),
        Err(error) => Err(format!("genie cannot get {what}. {error}")),
    }
}

fn fresh(path: &Path, now: SystemTime) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| now.duration_since(modified).unwrap_or_default() < FRESH)
}

fn text(bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn lines(bytes: Vec<u8>) -> Result<Vec<String>, String> {
    let mut reader = Cursor::new(bytes);
    let index = bbi::Index::read(&mut reader).map_err(|error| error.to_string())?;
    let mut found = Vec::new();
    for contig in index.names() {
        found.extend(
            index
                .lines(&mut reader, contig)
                .map_err(|error| error.to_string())?,
        );
    }
    Ok(found)
}

fn band_text(bytes: Vec<u8>, big: bool) -> Result<String, String> {
    let rows: Vec<(String, Band)> = if big {
        let mut rows = Vec::new();
        for line in lines(bytes)? {
            if let Some((contig, band)) =
                cytoband::parse(&line).map_err(|reason| reason.to_string())?
            {
                rows.push((contig.to_owned(), band));
            }
        }
        rows
    } else {
        let plain = gzip::inflate(&bytes).map_err(|error| error.to_string())?;
        cytoband::read(plain.as_slice()).map_err(|error| error.to_string())?
    };
    let mut written = Vec::new();
    cytoband::write(
        &mut written,
        rows.iter().map(|(contig, band)| (contig.as_str(), band)),
    )
    .map_err(|error| error.to_string())?;
    text(written)
}

fn gene_text(bytes: Vec<u8>, big: bool) -> Result<String, String> {
    let rows: Vec<(String, Feature)> = if big {
        let mut rows = Vec::new();
        for line in lines(bytes)? {
            if let Some((contig, feature)) =
                genepred::big(&line).map_err(|reason| reason.to_string())?
            {
                rows.push((contig.to_owned(), feature));
            }
        }
        rows
    } else {
        let plain = gzip::inflate(&bytes).map_err(|error| error.to_string())?;
        genepred::read(plain.as_slice()).map_err(|error| error.to_string())?
    };
    let mut written = Vec::new();
    bed::write_features(
        &mut written,
        rows.iter()
            .map(|(contig, feature)| (contig.as_str(), feature)),
    )
    .map_err(|error| error.to_string())?;
    text(written)
}

fn extras(folder: &Path) -> Result<Extras, String> {
    let open = |name: &str| {
        File::open(folder.join(name))
            .map(BufReader::new)
            .map_err(|error| error.to_string())
    };
    let sizes = contigs::sizes(open(SIZES)?).map_err(|error| error.to_string())?;
    let aliases = contigs::aliases(open(ALIASES)?).map_err(|error| error.to_string())?;
    let bands = match open(BANDS) {
        Ok(reader) => cytoband::read(reader).map_err(|error| error.to_string())?,
        Err(_) => Vec::new(),
    };
    Ok(Extras {
        sizes: sizes.into_iter().collect(),
        aliases,
        bands,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use genie_formats::gzip;

    use std::sync::atomic::AtomicBool;

    use super::{
        Annotation, Genome, SEQUENCE, STOPPED, all, download, find, offline, online, open, prepare,
        remove,
    };
    use crate::location::Location;
    use crate::scratch::Scratch;
    use crate::server::{Server, serve};

    const MONTH: Duration = Duration::from_hours(31 * 24);

    #[test]
    fn the_catalog_lists_ten_ucsc_genomes() {
        let genomes = all();
        let ids: Vec<&str> = genomes.iter().map(|genome| genome.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "hg38", "hg19", "hs1", "mm39", "mm10", "rn7", "danRer11", "dm6", "ce11", "sacCer3"
            ]
        );
        let hg38 = find("hg38").unwrap();
        assert_eq!(
            hg38.sequence,
            "https://hgdownload.soe.ucsc.edu/goldenPath/hg38/bigZips/hg38.2bit"
        );
        assert_eq!(
            hg38.sizes,
            "https://hgdownload.soe.ucsc.edu/goldenPath/hg38/bigZips/hg38.chrom.sizes"
        );
        assert_eq!(
            hg38.genes.url,
            "https://hgdownload.soe.ucsc.edu/goldenPath/hg38/database/ncbiRefSeqSelect.txt.gz"
        );
        assert_eq!(hg38.track, "RefSeq Select");
        let t2t = find("hs1").unwrap();
        assert_eq!(t2t.label, "T2T");
        assert!(t2t.genes.big && t2t.bands.as_ref().unwrap().big);
        assert!(
            t2t.genes
                .url
                .ends_with("GCA_009914755.4_T2T-CHM13v2.0.ncbiRefSeq.bb")
        );
        let rat = find("rn7").unwrap();
        assert!(rat.genes.url.ends_with("/rn7/database/ncbiRefSeq.txt.gz"));
        assert_eq!(rat.track, "NCBI RefSeq");
        assert_eq!(find("sacCer3").unwrap().bands, None);
        assert_eq!(find("nope"), None);
    }

    fn served(server: &Server) -> Genome {
        Genome {
            id: "test".to_owned(),
            label: "test".to_owned(),
            species: "Test".to_owned(),
            assembly: "T1".to_owned(),
            sequence: server.url("/t.2bit"),
            sizes: server.url("/t.chrom.sizes"),
            aliases: server.url("/t.chromAlias.txt"),
            bands: Some(Annotation {
                url: server.url("/cytoBandIdeo.txt.gz"),
                big: false,
            }),
            genes: Annotation {
                url: server.url("/genes.txt.gz"),
                big: false,
            },
            track: "Genes".to_owned(),
            megabytes: 1,
        }
    }

    fn files() -> Vec<(String, Vec<u8>)> {
        vec![
            ("/t.chrom.sizes".to_owned(), b"c\t4\n".to_vec()),
            (
                "/t.chromAlias.txt".to_owned(),
                b"# sequenceName\talias names\nc\tNC_1\n".to_vec(),
            ),
            (
                "/cytoBandIdeo.txt.gz".to_owned(),
                gzip::compress(b"c\t0\t2\tp1\tgneg\nc\t2\t4\tq1\tgpos50\n"),
            ),
            (
                "/genes.txt.gz".to_owned(),
                gzip::compress(
                    b"1\tNM_1\tNC_1\t+\t0\t4\t1\t3\t1\t0,\t4,\t0\tGENE1\tcmpl\tcmpl\t0,\n",
                ),
            ),
        ]
    }

    #[test]
    fn small_files_download_once_and_convert() {
        let server = serve(files());
        let scratch = Scratch::new("genome-small");
        let folder = scratch.file("marker", "").with_file_name("test");
        let now = SystemTime::now();
        let prepared = prepare(&served(&server), &folder, now).unwrap();
        assert_eq!(server.requests(), 4);
        assert_eq!(
            prepared.sequence,
            Location::parse(&server.url("/t.2bit")).unwrap()
        );
        assert_eq!(prepared.extras.sizes.get("c"), Some(&4));
        assert_eq!(prepared.extras.aliases, [vec!["c", "NC_1"]]);
        assert_eq!(prepared.extras.bands.len(), 2);
        let genes = fs::read_to_string(served(&server).genes_in(&folder)).unwrap();
        assert!(
            genes.starts_with("NC_1\t0\t4\tGENE1\t0\t+\t1\t3\t"),
            "{genes}"
        );
        prepare(&served(&server), &folder, now).unwrap();
        assert_eq!(server.requests(), 4);
        prepare(&served(&server), &folder, now + MONTH).unwrap();
        assert_eq!(server.requests(), 8);
    }

    const ACGT_2BIT: [u8; 39] = [
        0x43, 0x27, 0x41, 0x1A, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, b'c', 22, 0, 0, 0, 4, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x9C,
    ];

    #[test]
    fn genomes_open_as_references_with_their_extras() {
        let mut list = files();
        list.push(("/t.2bit".to_owned(), ACGT_2BIT.to_vec()));
        let server = serve(list);
        let scratch = Scratch::new("genome-open");
        let folder = scratch.file("marker", "").with_file_name("test");
        let reference = open(&served(&server), &folder, SystemTime::now()).unwrap();
        assert_eq!(reference.genome.as_deref(), Some("test"));
        assert_eq!(reference.assembly.find("NC_1"), Some(0));
        assert_eq!(reference.bands(0).len(), 2);
        assert_eq!(
            reference
                .fetch(0, genie_core::Region::between(0, 4))
                .unwrap(),
            b"ACGT"
        );
    }

    #[test]
    fn genomes_read_from_the_server_again_beside_an_offline_copy() {
        let mut list = files();
        list.push(("/t.2bit".to_owned(), ACGT_2BIT.to_vec()));
        let server = serve(list);
        let genome = served(&server);
        let scratch = Scratch::new("genome-online");
        let folder = scratch.file("marker", "").with_file_name("test");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(SEQUENCE), ACGT_2BIT).unwrap();
        let offline = open(&genome, &folder, SystemTime::now()).unwrap();
        assert_eq!(offline.location, Location::File(folder.join(SEQUENCE)));
        let reference = online(&genome, &folder).unwrap();
        assert_eq!(
            reference.location,
            Location::parse(&server.url("/t.2bit")).unwrap()
        );
        assert_eq!(reference.genome.as_deref(), Some("test"));
        assert_eq!(reference.assembly, offline.assembly);
        assert_eq!(
            reference
                .fetch(0, genie_core::Region::between(0, 4))
                .unwrap(),
            b"ACGT"
        );
    }

    #[test]
    fn offline_copies_download_resume_stop_and_go_away() {
        let sequence: Vec<u8> = (0..=255).cycle().take(20 << 20).collect();
        let mut list = files();
        list.push(("/t.2bit".to_owned(), sequence.clone()));
        let server = serve(list);
        let genome = served(&server);
        let scratch = Scratch::new("genome-offline");
        let folder = scratch.file("marker", "").with_file_name("test");
        let stop = AtomicBool::new(true);
        let error = download(&genome, &folder, &stop, |_, _| {}).unwrap_err();
        assert_eq!(error, STOPPED);
        assert!(!offline(&folder));
        fs::write(folder.join("sequence.2bit.part"), &sequence[..1_000]).unwrap();
        let stop = AtomicBool::new(false);
        let mut steps = Vec::new();
        download(&genome, &folder, &stop, |done, size| {
            steps.push((done, size));
        })
        .unwrap();
        assert!(offline(&folder));
        assert_eq!(fs::read(folder.join(SEQUENCE)).unwrap(), sequence);
        assert_eq!(steps.first(), Some(&(1_000 + (4 << 20), 20 << 20)));
        assert_eq!(steps.last(), Some(&(20 << 20, 20 << 20)));
        assert!(!folder.join("sequence.2bit.part").exists());
        remove(&folder).unwrap();
        assert!(!offline(&folder));
        remove(&folder).unwrap();
    }

    #[test]
    fn stale_files_stay_in_use_when_the_server_is_away() {
        let server = serve(files());
        let scratch = Scratch::new("genome-stale");
        let folder = scratch.file("marker", "").with_file_name("test");
        prepare(&served(&server), &folder, SystemTime::now()).unwrap();
        let mut away = served(&server);
        let gone = server.url("/gone");
        for url in [&mut away.sizes, &mut away.aliases, &mut away.genes.url] {
            url.clone_from(&gone);
        }
        away.bands.as_mut().unwrap().url.clone_from(&gone);
        let prepared = prepare(&away, &folder, SystemTime::now() + MONTH).unwrap();
        assert_eq!(prepared.extras.bands.len(), 2);
        let empty = scratch.file("marker2", "").with_file_name("empty");
        let error = prepare(&away, &empty, SystemTime::now()).unwrap_err();
        assert!(
            error.starts_with("genie cannot get the contig sizes of test."),
            "{error}"
        );
    }

    #[test]
    fn bigbed_annotations_and_offline_copies_are_used() {
        let genes = fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../genie-formats/tests/data/genes.bb"),
        )
        .unwrap();
        let mut list = files();
        list.push(("/genes.bb".to_owned(), genes));
        let server = serve(list);
        let mut genome = served(&server);
        genome.genes = Annotation {
            url: server.url("/genes.bb"),
            big: true,
        };
        genome.bands = Some(Annotation {
            url: server.url("/genes.bb"),
            big: true,
        });
        let scratch = Scratch::new("genome-big");
        let folder = scratch.file("marker", "").with_file_name("test");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("sequence.2bit"), b"").unwrap();
        let prepared = prepare(&genome, &folder, SystemTime::now()).unwrap();
        assert_eq!(
            prepared.sequence,
            Location::File(folder.join("sequence.2bit"))
        );
        let text = fs::read_to_string(genome.genes_in(&folder)).unwrap();
        assert!(text.contains("\tchrX_001\t"), "{text}");
        assert!(prepared.extras.bands.len() > 100);
    }
}
