use std::fs::File;
use std::path::Path;

use genie_core::Region;
use genie_core::alignment::{DUPLICATE, Filter};
use genie_formats::bgzf::Bgzf;
use genie_formats::{bai, bam};

use super::{Gather, Loaded, Options, with_suffix};

const MARGIN: u64 = 1_000;

#[derive(Debug)]
pub struct ReadFile {
    pub index: bai::Index,
    pub contigs: Vec<String>,
}

impl ReadFile {
    pub const SPAN: u64 = 30_000;

    pub fn open(path: &Path, name: &str) -> Result<Self, String> {
        let file = [
            with_suffix(path, ".bai"),
            path.with_extension("bai"),
            with_suffix(path, ".csi"),
        ]
        .into_iter()
        .find_map(|candidate| File::open(candidate).ok())
        .ok_or_else(|| {
            format!("There is no index {name}.bai. Index the file with samtools index.")
        })?;
        let index = bai::Index::read(file)
            .map_err(|error| format!("The index {name}.bai is not valid. {error}"))?;
        let file = File::open(path).map_err(|error| error.to_string())?;
        let header = bam::header(&mut Bgzf::new(file)).map_err(|error| error.to_string())?;
        let contigs = header
            .references
            .into_iter()
            .map(|(contig, _)| contig)
            .collect();
        Ok(Self { index, contigs })
    }

    pub fn load(
        &self,
        path: &Path,
        contig: &str,
        region: Region,
        options: Options,
        reference: impl FnOnce(Region) -> Result<Vec<u8>, String>,
    ) -> Result<Loaded, String> {
        let mut reader = File::open(path)
            .map(Bgzf::new)
            .map_err(|error| error.to_string())?;
        let id = self
            .contigs
            .iter()
            .position(|known| known == contig)
            .ok_or_else(|| format!("The file has no contig {contig}."))?;
        let filter = if options.duplicates {
            Filter::default().showing(DUPLICATE)
        } else {
            Filter::default()
        };
        let mut gather = Gather::new(region, id, options);
        bam::query(&self.index, &mut reader, id, region, |alignment| {
            if filter.keeps(&alignment) {
                gather.add(alignment);
            }
            Ok(())
        })
        .map_err(|error| error.to_string())?;
        let window = Region::between(
            region.start().saturating_sub(MARGIN),
            region.end().saturating_add(MARGIN),
        );
        let bases = reference(window)?;
        Ok(Loaded::reads(gather, bases, window.start()))
    }
}
