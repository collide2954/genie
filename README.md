# genie

genie is a genome browser for Windows, macOS and Linux. It uses the [iced](https://iced.rs) GUI library. The project page is at [collide2954.github.io/genie](https://collide2954.github.io/genie/).

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
- To use a genome without a network connection, click the genome label, then click **Use offline**. genie downloads the full sequence into its cache folder (human: about 835 MB). If the download stops, genie continues it the next time. Click **Remove offline copy** to delete the sequence. If the open genome reads from the offline copy, genie first changes it to the sequence on UCSC.
- Sessions and the recent list keep the genome.

## UCSC tracks

Click the tracks button (the layers icon) in the title bar to add a public track from the UCSC Genome Browser to the open genome.

| Genome | Genes | Variation | Conservation | Regulation |
| --- | --- | --- | --- | --- |
| hg38 | MANE Select, GENCODE V48 | ClinVar, dbSNP 155 common | phyloP 100-way, phastCons 100-way, GC percent | ENCODE cCREs |
| hg19 | GENCODE V48lift37 | ClinVar, dbSNP 155 common | phyloP 100-way, phastCons 100-way | |
| mm39 | GENCODE VM37 | EVA SNP 6 | phyloP 35-way, phastCons 35-way, GC percent | |
| mm10 | UCSC Genes | EVA SNP 6 | phyloP 60-way, phastCons 60-way, GC percent | ENCODE cCREs |
| T2T, rn7, danRer11 | | | GC percent | |
| dm6 | | | phyloP 124-way, GC percent | |
| ce11 | | | phyloP 135-way, GC percent | |

- genie reads these tracks in parts from the UCSC download server, as for other [files on the web](#files-on-the-web).
- The track gets the name from the list, for example ClinVar. A track that is open shows **Added** in the list.
- For yeast and for reference files, the list is empty and the button is off.
- Sessions keep these tracks with their names.

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

## Several loci

- Type two to four loci in the locus box, with a space between them, for example `BRCA1 BRCA2`. genie shows the loci side by side in panels of equal width.
- The top of each panel shows its locus and its ruler. Click the locus to show only that panel. Click × to close the panel. To show one panel again, type one locus.
- Drag, turn the wheel or drag on the ruler in a panel to move or zoom that panel. The arrow keys and + and - move and zoom all panels.
- Put the pointer on a feature, a read or a variant in a panel to see its data.
- Back, forward, region jumps and feature jumps move the first panel.
- Sessions, Ctrl+C (Cmd+C on macOS) and the `goto` command keep all loci. Images show all panels.

## Regions

- Click the bookmark button to show the region list. Push Ctrl+D (Cmd+D on macOS) to add the view to the list, or hold Shift and drag on the ruler to add a range.
- Click a region to go to it. Double-click a region to change its name, then push Enter.
- Import BED and Export BED read and write the list as a BED file. genie reads plain BED files up to 50 MB. It does not import regions on contigs that are not in the reference.
- Regions show as bands under the ruler. Put the pointer on a band to see its name, and click the band to go to the region.

## Copy

- Push Ctrl+C (Cmd+C on macOS) to copy the locus of the view, for example `chr1:1,001-2,000`. If the view has several loci, genie copies all of them, with a space between them. If you clicked a card to keep it open, Ctrl+C copies the text of the card.
- Right-click the ruler or a region in the region list, then click **Copy locus** or **Copy sequence**.
- **Copy sequence** copies the bases in FASTA format, with 60 bases on each line. The header is the locus, for example `>chr1:1001-2000`. If the sequence track shows the reverse strand, genie copies the reverse complement and adds `(-)` to the header. genie copies up to 1 Mb.

## Tracks

- Put the pointer on a feature, a read or a variant to see its data. Click to keep the card open.
- A card also shows the other fields of the file: bigBed fields with the descriptions from the file, GFF3 and GTF attributes, and the BED columns after column 12 (Column 13 and so on). A variant card shows the INFO entries with the descriptions from the `##INFO` lines of the header. genie does not show empty fields, fields with the value `none` or `.`, and bigBed fields that UCSC hides (names that start with `_`).
- A card shows up to 6 fields. Click to keep the card open and see all fields. If the card is taller than the window, scroll it. Ctrl+C (Cmd+C on macOS) copies the card with all fields.
- genie keeps the first 400 characters of the INFO column of each variant. If it cuts the INFO column, the card shows **More INFO**.
- A variant card shows QUAL and FILTER as the fields Quality and Filter.
- To filter a track, click a value in a card that you keep open. Then click **Keep only** or **Hide** the value, or for a number **At least** or **At most** the value. Each rule hides more features or variants. A value with commas passes when one of its items passes, for example the tags of a GENCODE transcript or the frequencies of a dbSNP variant.
- A filtered track shows the number of features or variants in view that it keeps, for example "12 of 586", at the top right. Click the number, or right-click the track, to see the rules. Click × to remove a rule.
- Click **Label with this field** in the same menu to name the features with that field, for example the gene type. **Label with name** in the track menu goes back.
- Filters and labels apply when the track shows features or variants. A bigBed track wider than 10 Mb shows its density without the filter. **Next feature**, **Previous feature** and the name search do not use the filter. Sessions and images keep the filters.
- Type a feature name or a variant ID in the locus box to go to it. The search ignores letter case. genie finds variant IDs in VCF files up to 50 MB. It also finds names in bigBed files that have a name index (`bedToBigBed -extraIndex=name`), and in other bigBed files up to 50 MB.
- BED and bigBed features use the colors of the file (itemRgb). Right-click the track and turn off "Use file colors" to use the track color.
- In a bigBed file of gene models (bigGenePred), each transcript shows its gene symbol. genie uses the field that the file describes as the gene symbol. If there is no such field, it uses `geneName2`, then `name2`. A track from the [UCSC track list](#ucsc-tracks) can set the field. For example, mm10 UCSC Genes uses `geneName`.
- In a variant track, the color of a site shows the change: the color of the new base for an SNV, purple for an insertion and dark grey for a deletion. A site that does not pass FILTER has a light color.
- Under the sites, each sample has a row: a light color for heterozygous, the full color for homozygous alternate and grey for homozygous reference. A sample with no call has no mark. If there are more than 100 samples, the rows are thin. If there are more than 300 samples, each row shows a group of samples with the strongest call of the group (homozygous alternate, then heterozygous, then homozygous reference), and the card names the samples of the row.
- A signal track has a bar for each pixel. The bar goes to the mean value, and a light color goes to the maximum value. Values below zero go down from a zero line. The range of the values in view is at the top right.
- Right-click a signal track to show a line instead of bars. Turn on "Shared scale" on two or more signal tracks to give them the same range.
- To fix the range of a signal track, right-click it, type the minimum and the maximum, then push Enter. The fields show the range in view. **Auto range** goes back to the range of the data in view. A track with a fixed range does not use "Shared scale".
- To show two or more signal tracks in one lane, right-click a signal track, then click a track under **Overlay on**. The lane shows each signal as a line in the color of its track, with one range for all. If two tracks have the same color, genie gives the added track a different color. To show a track in its own lane again, right-click the lane and click the track under **Separate**.
- Double-click a track name to change it, then push Enter. Push Escape to stop. If you delete the name, the track shows its file name again. The track menu also has **Rename**.
- Right-click a track to set its rows, its color or its place.
- Right-click a read track to show duplicate reads and soft clips, or to keep all reads.
- genie colors unusual read pairs: vermillion for an insert size that is larger than expected, blue for a smaller one, green for two mates on the same strand, purple for mates that point away from each other, and orange for a mate on a different chromosome. The expected insert sizes come from the loaded pairs. Right-click a read track to turn these colors off.
- Right-click a read track to sort the rows by the base at that position, or to group the reads by phase (HP tag) or by strand.
- The card of a paired read shows the mate position, the insert size, the pair orientation and the HP and RG tags.
- A read with MAPQ 0 is placed in more than one spot of the genome. genie shows it in a light color, and its card says "placed in more than one spot".
- A split read has other parts in the genome (SA tag). A dark mark shows the clipped end of the read. The card lists the other parts, for example "chr2:1,234,000 + · MAPQ 60". In a card that you keep open, click a part to go to it.
- A read that spans an intron (N in the CIGAR) shows a thin line over the intron. Above the reads, an arc joins the two ends of each splice junction. A higher and thicker arc has more reads. The color of the arc shows the strand: blue for forward, vermillion for reverse and grey if the strand is not known. The strand comes from the XS or ts tag of the reads. If the reads have no such tag, it comes from the bases at the two ends of the intron: GT-AG, GC-AG or AT-AC on the forward strand, or their reverse complements. If there is space, the arc shows its number of reads. The count includes all reads, also the reads that down-sampling hides. Put the pointer on an arc to see its position, reads, strand and intron length. A collapsed read track shows the coverage and the arcs.
- Long reads can have base modifications (MM and ML tags), for example 5mC from nanopore or PacBio. Right-click a read track and turn on **Color by modification**. The reads become grey, and each call has a color: vermillion for modified, blue for not modified, and a light color for a probability near 50%. Mismatches stay on top. When you zoom out, a pixel shows the mean probability of its calls. The card of a read counts the calls for each modification, for example "Modified 5mC 34 of 120" (calls with a probability of 50% or more).
- Right-click the sequence to show the reverse strand and 3 or 6 reading frames.
- Drag a track name to move the track.
- Drag the bottom edge of a track to set its height. Double-click the edge to show all rows again.
- If a track has more than 200,000 shapes to draw in the view, it shows "There is too much to draw here. Zoom in." in place of the data.

## Sessions

To save the reference, the tracks (with their names, colors, ranges and overlays), the view and the regions as a session, push Ctrl+S (Cmd+S on macOS) or click the save button. To open a session, open its `.genie` file. genie keeps the last session when you close it, and the start view can restore it.

If a file is in the folder of the session file, the session file has its path relative to that folder.

## Export

To save the view as an image, push Ctrl+E (Cmd+E on macOS) or click the camera button. The image shows the locus, the ruler and all tracks, also the tracks that are not on the screen. If the view has several loci, the image shows each panel with its locus and its ruler. The image uses the current theme.

- If the file name ends with `.svg`, genie saves an SVG image. The SVG has shapes and text that you can edit, for example in Inkscape or Illustrator. The text uses the fonts Inter and JetBrains Mono. If a computer does not have these fonts, it uses similar fonts.
- Other names give a PNG image. The scale of a PNG image is 2. Its maximum height is 16,384 px. If the view is taller, the image shows only the top part, and genie tells you.

## Images from the command line

To save one image for each region in a BED file, without a window:

```bash
cargo run -p genie -- snapshot work.genie regions.bed pictures
```

- genie opens the session (the genome or reference, the tracks and their settings). Then it goes to each region and saves a PNG file in the folder. If the folder does not exist, genie makes it. Each image shows one region, also if the session has several loci.
- The name of an image is the name of the region (BED column 4) or the locus, for example `chr1_1001-2000.png`. If two regions have the same name, genie adds `-2`, `-3` and so on.
- `--width POINTS` sets the width of the view, from 200 to 4,096 (default 1,280). A PNG image has two pixels for each point. `--theme dark` uses the dark theme. The default is light. `--format svg` saves SVG images. The default is PNG.
- genie writes one line for each region. If genie cannot make an image for a region, it writes the reason and continues with the next region.
- The exit code is 0 if genie saved all images, 1 if one or more images are missing, and 2 if the command is not correct.

## Commands from other programs

While genie runs, programs on the same computer (for example R, Python or Jupyter) can control it through port 60151, as with the batch port of IGV. Send one command on each line. genie answers each line with `OK` or with the reason for a problem. It answers when the work is complete, for example when a new track is open and its data is loaded.

| Command | Result |
| --- | --- |
| `genome hg38`, `genome /data/genome.fa` | Opens a genome from the list or a reference file |
| `load /data/reads.bam name=Tumor` | Adds a track. `name=` sets the track name. |
| `goto chr1:1,000-2,000`, `goto BRCA1`, `goto BRCA1 BRCA2` | Goes to a locus or a feature name. Two to four loci show side by side. |
| `region chr1 1001 2000 promoter` | Adds a region to the region list. The start and the end are 1-based and are in the region. |
| `snapshotDirectory /data/pictures` | Sets the folder for snapshots |
| `snapshot BRCA1.png` | Saves the view as an image in that folder: SVG for a name that ends with `.svg`, else PNG. Without a name, the image is a PNG named after the locus. |
| `expand`, `squish`, `collapse` | Sets the rows of all tracks, or of one track, for example `squish Tumor` |
| `new` | Removes all tracks and regions |
| `echo` | Answers `echo` |

```python
import socket

with socket.create_connection(("127.0.0.1", 60151)) as connection:
    answers = connection.makefile("r")
    for line in ["goto BRCA1", "snapshotDirectory /tmp", "snapshot brca1.png"]:
        connection.sendall((line + "\n").encode())
        print(answers.readline().strip())
```

- Put a path that has spaces between double quotes: `load "/data/my reads.bam"`.
- genie accepts `maxPanelHeight` and `setSleepInterval`, but they have no effect.
- Only programs on this computer can connect. genie does not answer web (HTTP) requests, so web pages cannot send commands.
- To close the port, open the settings and set **Commands** to Off. If a different program (for example IGV) uses port 60151, genie shows a message and does not take commands.

## Settings

The gear button opens the settings: UI scale (90 % to 150 %), density, motion and the port for [commands from other programs](#commands-from-other-programs). genie keeps the settings and the theme in its configuration folder.

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
