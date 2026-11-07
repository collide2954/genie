# genie

genie is a genome browser for Windows, macOS and Linux. It uses the [iced](https://iced.rs) GUI library.

genie is not complete. There is no release. Each push to `main` makes a test build for each operating system. The builds are in the artifacts of the CI runs for 30 days.

## Run

Install [rustup](https://rustup.rs). rustup installs the toolchain version in `rust-toolchain.toml`.

```bash
cargo run -p genie
```

## Files

| Kind | Formats |
| --- | --- |
| Reference sequence | FASTA (`.fa`, `.fasta`, `.fna`, `.fas`), FASTA compressed with bgzip (`.fa.gz`), UCSC 2bit (`.2bit`) |
| Feature track | BED and GFF3: plain files up to 50 MB, or files compressed with bgzip and indexed with tabix |
| Alignment track | BAM, sorted by position and indexed with samtools |
| Variant track | VCF: plain files up to 50 MB, or files compressed with bgzip and indexed with tabix. genie does not read BCF. |
| Signal track | bigWig (`.bw`); bedGraph: plain files up to 50 MB, or files compressed with bgzip and indexed with tabix |
| Session | genie session (`.genie`) |

Open a reference sequence first. Then open tracks or drop them on the window. The index (`.tbi`, `.csi` or `.bai`) must be next to the track file.

If a FASTA file has no index (`.fai`, and `.gzi` for bgzip), genie makes the index and keeps it in its cache folder. For a large file, this can take some seconds. To make the indexes before you open the file:

```bash
bgzip genome.fa && samtools faidx genome.fa.gz
```

To prepare a large BED track:

```bash
sort -k1,1 -k2,2n genes.bed | bgzip > genes.bed.gz && tabix -p bed genes.bed.gz
```

For GFF3, sort by columns 1 and 4 and use `tabix -p gff`.

To prepare a BAM track:

```bash
samtools sort -o reads.bam reads.sam && samtools index reads.bam
```

genie shows reads when the view is 30 kb or less. It hides QC-fail and secondary reads. By default, it also hides duplicate reads and soft clips, and it keeps 100 reads for each 50 bp.

To prepare a large VCF track:

```bash
bgzip calls.vcf && tabix -p vcf calls.vcf.gz
```

genie shows variants when the view is 1 Mb or less.

To prepare a large bedGraph track:

```bash
sort -k1,1 -k2,2n signal.bedGraph | bgzip > signal.bedGraph.gz && tabix -p bed signal.bedGraph.gz
```

genie shows bigWig tracks and plain bedGraph tracks at all zoom levels. It shows a bedGraph track that has a tabix index when the view is 10 Mb or less. For the best speed, convert bedGraph to bigWig with bedGraphToBigWig.

## Tracks

- Put the pointer on a feature, a read or a variant to see its data. Click to keep the card open.
- Type a feature name or a variant ID in the locus box to go to it. genie finds variant IDs in VCF files up to 50 MB.
- In a variant track, the color of a site shows the change: the color of the new base for an SNV, purple for an insertion and dark grey for a deletion. A site that does not pass FILTER has a light color.
- Under the sites, each sample has a row: a light color for heterozygous, the full color for homozygous alternate and grey for homozygous reference. A sample with no call has no mark. If there are more than 100 samples, the rows are thin.
- A signal track has a bar for each pixel. The bar goes to the mean value, and a light color goes to the maximum value. Values below zero go down from a zero line. The range of the values in view is at the top right.
- Right-click a signal track to show a line instead of bars. Turn on "Shared scale" on two or more signal tracks to give them the same range.
- Right-click a track to set its rows, its color or its place.
- Right-click a read track to show duplicate reads and soft clips, or to keep all reads.
- Right-click the sequence to show the reverse strand and 3 or 6 reading frames.
- Drag a track name to move the track.
- Drag the bottom edge of a track to set its height. Double-click the edge to show all rows again.

## Sessions

To save the reference, the tracks and the view as a session, push Ctrl+S (Cmd+S on macOS) or click the save button. To open a session, open its `.genie` file. genie keeps the last session when you close it, and the start view can restore it.

If a file is in the folder of the session file, the session file has its path relative to that folder.

## Export

To save the view as a PNG image, push Ctrl+E (Cmd+E on macOS) or click the camera button. The image shows the locus, the ruler and all tracks, also the tracks that are not on the screen. The scale of the image is 2, and the image uses the current theme.

## Settings

The gear button opens the settings: UI scale (90 % to 150 %), density and motion. genie keeps the settings and the theme in its configuration folder.

## Diagnostics

genie writes warnings to stderr. To see more, set `GENIE_LOG` to `info`, `debug` or `trace`:

```bash
GENIE_LOG=debug cargo run -p genie
```

In a debug build, F12 shows the time of each update and view, the frame time and the use of the caches.

## Test

```bash
cargo test --workspace
```

## Crates

| Crate | Contents |
| --- | --- |
| `genie-core` | Genomic coordinates, loci, alignments and the viewport |
| `genie-formats` | Parsers and writers for genomic and image file formats |
| `genie-render` | Track layout and draw primitives |
| `genie` | The iced application |

## License

Apache-2.0. Refer to [LICENSE](LICENSE).

The fonts in `crates/genie/assets/fonts` (Inter, JetBrains Mono) use the SIL Open Font License 1.1. The license texts are in the same folder.
