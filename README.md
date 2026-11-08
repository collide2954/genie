# genie

genie is a genome browser for Windows, macOS and Linux. It uses the [iced](https://iced.rs) GUI library.

genie is not complete. There is no release. Each push to `main` makes a test build for each operating system. The builds are in the artifacts of the CI runs for 30 days.

## Run

Install [rustup](https://rustup.rs). rustup installs the toolchain version in `rust-toolchain.toml`.

```bash
cargo run -p genie
```

## Genomes

The start view lists ten genomes. Click a genome to open it with its gene track and its chromosome bands.

| Species | Genome | Assembly | Gene track |
| --- | --- | --- | --- |
| Human | hg38 | GRCh38 | RefSeq Select |
| Human | hg19 | GRCh37 | RefSeq Select |
| Human | T2T | T2T-CHM13v2.0 | NCBI RefSeq |
| Mouse | mm39 | GRCm39 | RefSeq Select |
| Mouse | mm10 | GRCm38 | RefSeq Select |
| Rat | rn7 | mRatBN7.2 | NCBI RefSeq |
| Zebrafish | danRer11 | GRCz11 | NCBI RefSeq |
| Fly | dm6 | BDGP6 | NCBI RefSeq |
| Worm | ce11 | WBcel235 | NCBI RefSeq |
| Yeast | sacCer3 | R64 | NCBI RefSeq |

- genie gets the genome files from the UCSC Genome Browser download server. It reads the sequence over the web, as for other [files on the web](#files-on-the-web).
- RefSeq Select has one transcript for each gene. NCBI RefSeq has all transcripts.
- Type a gene name, for example BRCA1, in the locus box to go to the gene.
- The overview strip shows the chromosome bands. Put the pointer on a band to see its name. If a genome has no band data, the strip is a plain bar.
- genie uses the contig names of the genome. A track that uses `1`, `NC_000001.11` or `CM000663.2` shows on `chr1` of hg38.
- The label left of the locus box shows the open genome. Click the label to open a different genome or a reference file.
- genie keeps the small genome files (contig names, bands and genes) in its cache folder. It gets them again after 30 days.
- To use a genome without a network connection, click the genome label, then click **Use offline**. genie downloads the full sequence into its cache folder (human: about 835 MB). If the download stops, genie continues it the next time. Click **Remove offline copy** to delete the sequence.
- Sessions and the recent list keep the genome.

## Files

| Kind | Formats |
| --- | --- |
| Reference sequence | FASTA (`.fa`, `.fasta`, `.fna`, `.fas`), FASTA compressed with bgzip (`.fa.gz`), UCSC 2bit (`.2bit`) |
| Feature track | BED, GFF3 and GTF: plain files up to 50 MB, or files compressed with bgzip and indexed with tabix; bigBed (`.bb`) |
| Alignment track | BAM, sorted by position and indexed with samtools |
| Variant track | VCF: plain files up to 50 MB, or files compressed with bgzip and indexed with tabix. genie does not read BCF. |
| Signal track | bigWig (`.bw`); bedGraph: plain files up to 50 MB, or files compressed with bgzip and indexed with tabix |
| Session | genie session (`.genie`) |

Open a genome or a reference sequence first. Then open tracks or drop them on the window. The index (`.tbi`, `.csi` or `.bai`) must be next to the track file.

If a FASTA file has no index (`.fai`, and `.gzi` for bgzip), genie makes the index and keeps it in its cache folder. For a large file, this can take some seconds. To make the indexes before you open the file:

```bash
bgzip genome.fa && samtools faidx genome.fa.gz
```

To prepare a large BED track:

```bash
sort -k1,1 -k2,2n genes.bed | bgzip > genes.bed.gz && tabix -p bed genes.bed.gz
```

For GFF3 and GTF, sort by columns 1 and 4 and use `tabix -p gff`:

```bash
sort -k1,1 -k4,4n genes.gtf | bgzip > genes.gtf.gz && tabix -p gff genes.gtf.gz
```

genie shows bigBed features when the view is 10 Mb or less. Wider views show the feature density from the zoom levels of the file.

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

## Files on the web

genie can open reference sequences and tracks from a web server. To open a file from the web:

1. Push Ctrl+U (Cmd+U on macOS) or click the link button.
2. Type the address of the file. The address must start with `https://` or `http://`.
3. Push Enter.

You can also give addresses as arguments:

```bash
cargo run -p genie -- https://example.org/data/genome.2bit https://example.org/data/reads.bam
```

- genie reads only the parts of a file that the view needs. The server must send parts of files (HTTP range requests). If the server sends only full files, genie opens only files of 4 MB or less.
- The indexes must be on the same server. The address of an index is the address of the file with the index extension, for example `reads.bam.bai`. genie does not make indexes for FASTA files from the web.
- genie reads plain BED, GFF3, GTF, VCF and bedGraph files completely. The limit is 50 MB.
- For files from the web, the locus box finds names only in bigBed files that have a name index.
- genie keeps up to 64 MB of data from the web in memory. It does not write this data to disk.
- genie uses the certificates of the operating system for HTTPS. A connection to an `http://` address is not secure. genie does not send user names, passwords or cookies.
- Sessions and the recent list keep the addresses of files from the web. genie cannot open a session file from the web.

## Navigation

- Drag the tracks to move along the sequence. Turn the wheel with Ctrl (Cmd on macOS) to zoom. Push the arrow keys to move and + or - to zoom.
- Drag on the ruler to zoom to that range. Push Escape to stop the drag.
- To go back to an earlier view, push Alt+Left (Cmd+[ on macOS), push the back button of the mouse or click the back arrow left of the locus box. Alt+Right (Cmd+] on macOS) goes forward again. genie keeps the last 100 jumps: the locus box, name search, ruler drags, region and feature jumps and changes of contig. Moves and zooms do not add a step.
- Push Ctrl+F or Ctrl+B (Cmd on macOS) to go to the next or the previous feature or variant of the track under the pointer. The search continues on the next contigs. The track menu has the same items.
- genie sorts the contigs. First are the numbered chromosomes in number order (1, 2 … 22, also arms such as 2L and 2R, and Roman numbers such as I to XVI). Then come X, Y, W and Z, and then M. The other contigs follow in the order of the file. The region list and the feature search use this order.

## Regions

- Click the bookmark button to show the region list. Push Ctrl+D (Cmd+D on macOS) to add the view to the list, or hold Shift and drag on the ruler to add a range.
- Click a region to go to it. Double-click a region to change its name, then push Enter.
- Import BED and Export BED read and write the list as a BED file. genie reads plain BED files up to 50 MB. It does not import regions on contigs that are not in the reference.
- Regions show as bands under the ruler. Put the pointer on a band to see its name, and click the band to go to the region.

## Tracks

- Put the pointer on a feature, a read or a variant to see its data. Click to keep the card open.
- Type a feature name or a variant ID in the locus box to go to it. The search ignores letter case. genie finds variant IDs in VCF files up to 50 MB. It also finds names in bigBed files that have a name index (`bedToBigBed -extraIndex=name`), and in other bigBed files up to 50 MB.
- BED and bigBed features use the colors of the file (itemRgb). Right-click the track and turn off "Use file colors" to use the track color.
- In a variant track, the color of a site shows the change: the color of the new base for an SNV, purple for an insertion and dark grey for a deletion. A site that does not pass FILTER has a light color.
- Under the sites, each sample has a row: a light color for heterozygous, the full color for homozygous alternate and grey for homozygous reference. A sample with no call has no mark. If there are more than 100 samples, the rows are thin.
- A signal track has a bar for each pixel. The bar goes to the mean value, and a light color goes to the maximum value. Values below zero go down from a zero line. The range of the values in view is at the top right.
- Right-click a signal track to show a line instead of bars. Turn on "Shared scale" on two or more signal tracks to give them the same range.
- Right-click a track to set its rows, its color or its place.
- Right-click a read track to show duplicate reads and soft clips, or to keep all reads.
- genie colors unusual read pairs: vermillion for an insert size that is larger than expected, blue for a smaller one, green for two mates on the same strand, purple for mates that point away from each other, and orange for a mate on a different chromosome. The expected insert sizes come from the loaded pairs. Right-click a read track to turn these colors off.
- Right-click a read track to sort the rows by the base at that position, or to group the reads by phase (HP tag) or by strand.
- The card of a paired read shows the mate position, the insert size, the pair orientation and the HP and RG tags.
- Right-click the sequence to show the reverse strand and 3 or 6 reading frames.
- Drag a track name to move the track.
- Drag the bottom edge of a track to set its height. Double-click the edge to show all rows again.

## Sessions

To save the reference, the tracks, the view and the regions as a session, push Ctrl+S (Cmd+S on macOS) or click the save button. To open a session, open its `.genie` file. genie keeps the last session when you close it, and the start view can restore it.

If a file is in the folder of the session file, the session file has its path relative to that folder.

## Export

To save the view as a PNG image, push Ctrl+E (Cmd+E on macOS) or click the camera button. The image shows the locus, the ruler and all tracks, also the tracks that are not on the screen. The scale of the image is 2, and the image uses the current theme. The maximum height of the image is 16,384 px. If the view is taller, the image shows only the top part, and genie tells you.

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
| `genie-formats` | Parsers and writers for genomic and image file formats, and an HTTP client for files on the web |
| `genie-render` | Track layout and draw primitives |
| `genie` | The iced application |

## License

Apache-2.0. Refer to [LICENSE](LICENSE).

The fonts in `crates/genie/assets/fonts` (Inter, JetBrains Mono) use the SIL Open Font License 1.1. The license texts are in the same folder.
