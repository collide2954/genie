# genie

genie is a genome browser for Windows, macOS and Linux. It uses the [iced](https://iced.rs) GUI library.

genie is not complete. There is no release.

## Run

Install [rustup](https://rustup.rs). rustup installs the toolchain version in `rust-toolchain.toml`.

```bash
cargo run -p genie
```

## Files

| Kind | Formats |
| --- | --- |
| Reference sequence | FASTA (`.fa`, `.fasta`, `.fna`, `.fas`), UCSC 2bit (`.2bit`) |
| Feature track | BED and GFF3, compressed with bgzip and indexed with tabix |
| Alignment track | BAM, sorted by position and indexed with samtools |

Open a reference sequence first. Then open tracks or drop them on the window. The index (`.tbi` or `.bai`) must be next to the track file.

To prepare a BED track:

```bash
sort -k1,1 -k2,2n genes.bed | bgzip > genes.bed.gz && tabix -p bed genes.bed.gz
```

For GFF3, sort by columns 1 and 4 and use `tabix -p gff`.

To prepare a BAM track:

```bash
samtools sort -o reads.bam reads.sam && samtools index reads.bam
```

genie shows reads when the view is 30 kb or less. It hides duplicate, QC-fail and secondary reads.

## Test

```bash
cargo test --workspace
```

## Crates

| Crate | Contents |
| --- | --- |
| `genie-core` | Genomic coordinates, loci and the viewport |
| `genie-formats` | Parsers for genomic file formats |
| `genie-render` | Track layout and draw primitives |
| `genie` | The iced application |

## License

Apache-2.0. Refer to [LICENSE](LICENSE).

The fonts in `crates/genie/assets/fonts` (Inter, JetBrains Mono) use the SIL Open Font License 1.1. The license texts are in the same folder.
