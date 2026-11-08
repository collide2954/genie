#[path = "support/server.rs"]
mod server;

use std::fs::{self, File};
use std::path::PathBuf;

use genie_core::Region;
use genie_formats::bgzf::Bgzf;
use genie_formats::http::Url;
use genie_formats::remote::Remote;
use genie_formats::{bai, bam, bbi, tabix, vcf};

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

struct Files {
    server: server::Server,
}

impl Files {
    fn serve(names: &[&str]) -> Self {
        let files = names
            .iter()
            .map(|name| (format!("/{name}"), fs::read(data(name)).unwrap()))
            .collect();
        Self {
            server: server::serve(files),
        }
    }

    fn open(&self, name: &str) -> Remote {
        Remote::open(&Url::parse(&self.server.url(&format!("/{name}"))).unwrap()).unwrap()
    }
}

#[test]
fn remote_bam_queries_equal_local_ones() {
    let files = Files::serve(&["reads.bam", "reads.bam.bai"]);
    let remote_index = bai::Index::read(files.open("reads.bam.bai")).unwrap();
    let local_index = bai::Index::read(File::open(data("reads.bam.bai")).unwrap()).unwrap();
    let region = Region::between(1_000, 41_000);
    let mut remote = Vec::new();
    let mut reader = Bgzf::new(files.open("reads.bam"));
    bam::query(&remote_index, &mut reader, 0, region, |read| {
        remote.push(read);
        Ok(())
    })
    .unwrap();
    let mut local = Vec::new();
    let mut reader = Bgzf::new(File::open(data("reads.bam")).unwrap());
    bam::query(&local_index, &mut reader, 0, region, |read| {
        local.push(read);
        Ok(())
    })
    .unwrap();
    assert_ne!(local.len(), 0);
    assert_eq!(remote, local);
}

#[test]
fn remote_vcf_queries_equal_local_ones() {
    let files = Files::serve(&["variants.vcf.gz", "variants.vcf.gz.tbi"]);
    let remote_index = tabix::Index::read(files.open("variants.vcf.gz.tbi")).unwrap();
    let local_index = tabix::Index::read(File::open(data("variants.vcf.gz.tbi")).unwrap()).unwrap();
    let region = Region::between(0, 200_000);
    let remote = vcf::query(
        &remote_index,
        &mut Bgzf::new(files.open("variants.vcf.gz")),
        "chr2",
        region,
    )
    .unwrap();
    let local = vcf::query(
        &local_index,
        &mut Bgzf::new(File::open(data("variants.vcf.gz")).unwrap()),
        "chr2",
        region,
    )
    .unwrap();
    assert_ne!(local.len(), 0);
    assert_eq!(remote, local);
}

#[test]
fn remote_bigwig_and_bigbed_reads_equal_local_ones() {
    let files = Files::serve(&["signal.bw", "genes.bb"]);
    for (name, contig) in [("signal.bw", "chr1"), ("genes.bb", "chr2")] {
        let mut remote = files.open(name);
        let mut local = File::open(data(name)).unwrap();
        let remote_index = bbi::Index::read(&mut remote).unwrap();
        let local_index = bbi::Index::read(&mut local).unwrap();
        let region = Region::between(0, 2_000_000);
        for step in [1, 1_024, 65_536] {
            let far = remote_index
                .summaries(&mut remote, contig, region, step)
                .unwrap();
            let near = local_index
                .summaries(&mut local, contig, region, step)
                .unwrap();
            assert_ne!(near.len(), 0, "{name} {step}");
            assert_eq!(far, near, "{name} {step}");
        }
        if remote_index.is_bed() {
            let far = remote_index.features(&mut remote, contig, region).unwrap();
            let near = local_index.features(&mut local, contig, region).unwrap();
            assert_eq!(far, near, "{name}");
        }
    }
}
