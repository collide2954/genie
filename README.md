# genie

genie is a genome browser for Windows, macOS and Linux. It uses the [iced](https://iced.rs) GUI library. The project page is at [collide2954.github.io/genie](https://collide2954.github.io/genie/).

## Install

Download the package for your system from the [latest release](https://github.com/collide2954/genie/releases/latest).

| System | Package |
| --- | --- |
| Windows 10 or 11 (x64) | `genie-0.3.1-windows-x64-setup.exe` |
| macOS 11 or later (Apple silicon and Intel) | `genie-0.3.1-macos-universal.dmg` |
| Linux (x86_64) | `genie-0.3.1-linux-x86_64.AppImage` or `genie_0.3.1_amd64.deb` |

The packages are not signed yet, so the system asks before genie opens the first time:

- Windows: if SmartScreen shows "Windows protected your PC", click **More info**, then **Run anyway**. The installer needs no administrator rights. It adds genie to the Start menu and opens `.genie` session files in genie.
- macOS: open the disk image and drag genie to Applications. Open genie. If macOS stops it, open **System Settings > Privacy & Security** and click **Open Anyway**.
- Linux: make the AppImage executable (`chmod +x genie-0.3.1-linux-x86_64.AppImage`) and run it. Some systems need FUSE 2 (the `libfuse2` package). Or install the .deb with `sudo apt install ./genie_0.3.1_amd64.deb`.

Each push to `main` also makes a test build for each operating system. The builds are in the artifacts of the CI runs for 30 days.

## Build from source

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

### NCBI assemblies

UCSC GenArk has assembly hubs for thousands of NCBI assemblies of other species. To open one, click **Other NCBI assemblies…** in the start view, or click the genome label and then **NCBI assembly…**. The palette has **Open an NCBI assembly…** too.

- Type a species, a common name or an accession, for example `Gallus gallus`, `chicken` or `GCF_016699485.2`. The list shows the assemblies that have all the words. Accessions that start with the words come first, then species and common names that start with them. Outdated assemblies, which UCSC keeps in its legacy set, come after the others and show **outdated**.
- Click an assembly. genie opens its GenArk hub, then the genome of the hub. The hub dialog then shows the tracks of the hub, such as the genes of NCBI RefSeq.
- An accession with its version (GCA or GCF, nine digits, a dot and the version) also opens from **Open URL…** and **Open a track hub…**, and from the dialog when it is not in the list.
- genie gets the list of GenArk assemblies (about 6 MB) from UCSC the first time you open the dialog, and keeps it in its cache folder for 7 days. Without a network connection, genie uses the list that it has.
- The contigs of a GenArk genome have the names in its 2bit file, often the NCBI accessions of the sequences.

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
- For yeast and for reference files, the menu has no UCSC tracks. It always has the [track hubs](#track-hubs).
- Sessions keep these tracks with their names.

## Track hubs

A track hub is a set of track files on a web server, with a `hub.txt` file that names them. The UCSC Genome Browser uses track hubs. To open a track hub:

1. Click the tracks button, then click **Open a track hub…**. The genie menu and the palette (Ctrl+K) also have this item.
2. Type the address of the `hub.txt` file, then push Enter.

A dialog shows the tracks of the hub for the open genome, in their groups: super tracks, composite tracks and views. Type words to find a track. Click a track to add it. Click it again to remove it.

- Open URL (Ctrl+U) also opens a hub when the address ends with `.txt`. To open a hub on your computer, open its `hub.txt` file.
- genie reads the hub, genomes and trackDb files of the hub, and the files that their `include` lines name. It uses the genome of the hub that has the name of the open genome, for example hg38, or the name of the reference file.
- genie adds tracks of these types: bigBed, bigWig and the other UCSC big formats (for example bigGenePred and bigNarrowPeak), bam, cram, vcfTabix and hic. genie uses the type and the index that the hub gives, so the file names can be anything. The list shows tracks of other types, for example longTabix, but genie cannot add them.
- A track gets its short label from the hub as its name.
- If the hub has no tracks for the open genome, the dialog lists the genomes of the hub. Click a genome to open it with the tracks of the hub. genie opens the 2bit sequence of an assembly hub, or the UCSC genome with the same name.
- The dialog shows the first 500 rows. Type words to find the other tracks.
- The tracks button lists the hubs of the session and the 8 hubs that you opened last.
- Sessions keep their hubs, and the type and the index of each track from a hub.

## Files

| Kind | Formats |
| --- | --- |
| Reference sequence | FASTA (`.fa`, `.fasta`, `.fna`, `.fas`), FASTA compressed with bgzip (`.fa.gz`), UCSC 2bit (`.2bit`) |
| Feature track | BED, GFF3 and GTF (plain, gzip or bgzip); bigBed (`.bb`) |
| Alignment track | BAM; CRAM 3.0 and 3.1; PAF from minimap2, wfmash and other aligners (`.paf`); PSL from BLAT (`.psl`). PAF and PSL are plain, gzip or bgzip |
| Variant track | VCF (plain, gzip or bgzip); BCF (`.bcf`) with its CSI index (`.bcf.csi`) |
| Signal track | bigWig (`.bw`); bedGraph (plain, gzip or bgzip); bedMethyl from modkit (plain, gzip or bgzip) |
| Interaction track | bigInteract (`.bb`); BEDPE (`.bedpe`, plain or gzip, up to 50 MB); loop lists of Juicer and HiCCUPS (`.txt`, plain or gzip) |
| Junction track | STAR splice junctions (`SJ.out.tab`); junction BED files of TopHat and regtools (`junctions.bed`, or a BED file with the track line `track name=junctions` or `graphType=junctions`). Plain or gzip, up to 50 MB |
| Hi-C track | Juicer `.hic`, versions 6 to 9; cooler `.cool` and `.mcool`, as from the 4D Nucleome project |
| Fragment track | Fragment files of single-cell ATAC-seq from Cell Ranger ATAC (`fragments.tsv.gz`) with their tabix index, or plain or gzip up to 256 MB (see [Single-cell](#single-cell)) |
| Table track | Tab-separated tables compressed with bgzip and indexed with `tabix -s -b -e`, for example the CADD scores (`.tsv.gz`, `.txt.gz` and others, with `.tbi` or `.csi`) |
| GWAS track | Summary statistics of the GWAS Catalog, PLINK 2 (`.glm.linear`, `.glm.logistic`), REGENIE (`.regenie`), SAIGE and BOLT-LMM: tab or space separated, plain or gzip up to 1 GB, or bgzip with a tabix index |
| Copy number track | SEG (`.seg`, as from TCGA, DNAcopy or GISTIC); CNVkit (`.cns`, `.cnr`); GATK ModelSegments (`.seg`). Plain or gzip, up to 50 MB |
| GenBank and EMBL | GenBank (`.gb`, `.gbk`, `.gbff`, `.genbank`) and EMBL (`.embl`, `.emb`) files, plain or gzip, up to 512 MB: the sequence and a feature track (see [GenBank and EMBL files](#genbank-and-embl-files)) |
| Motifs | MEME (`.meme`), JASPAR (`.jaspar`, `.pfm`, and the `.txt` files of JASPAR) and HOMER (`.motif`) files of DNA motifs, plain or gzip, up to 64 MB (see [Motif sites](#motif-sites)) |
| Sample table | Tab or comma separated text with a header line (`.tsv`, `.csv`, `.txt`, `.panel` from the 1000 Genomes Project); clinical files from cBioPortal; IGV sample information files; PLINK `.fam` and `.ped`. Plain or gzip, up to 64 MB |
| Cell table | The cells of single-cell data, as the meta.data of Seurat or the obs of Scanpy written as CSV or TSV, plain or gzip, up to 256 MB. Open it from the menu of a track (see [Single-cell](#single-cell)) |
| Session | genie session (`.genie`) |

Open a genome or a reference sequence first. Then open tracks or drop them on the window.

If a FASTA file has no index (`.fai`, and `.gzi` for bgzip), genie makes the index and keeps it in its cache folder. For a large file, this can take some seconds. To make the indexes before you open the file:

```bash
bgzip genome.fa && samtools faidx genome.fa.gz
```

### Files without an index

If a track file has an index next to it (`.tbi`, `.csi`, `.bai` or `.crai`), genie uses that index. If the file has no index, genie prepares the file in its cache folder:

- A sorted BAM file, a CRAM file or a sorted bgzip file gets an index. genie does not copy the file.
- A BCF file without its CSI index opens if it is 50 MB or less: genie reads all of it. Index larger BCF files with `bcftools index`.
- A plain or gzip file gets a copy compressed with bgzip, and an index. Plain files up to 50 MB open directly, without a copy.
- A file that is not sorted by position gets a sorted copy, for example a GTF file from GENCODE or Ensembl, or a BAM file from an aligner. genie sorts on disk, so the size of the file is not limited by memory.

While genie prepares a file, the track shows the step and the progress, for example "Sorting genes.gtf.gz… 45 %". The track shows its data when the work is complete. To stop the work, click × on the track.

genie keeps the copies and indexes until you clear them. The settings panel shows the space that they use. Click **Clear** to remove them. genie keeps the files of open tracks. If a track file changes, genie prepares it again.

You can also prepare files yourself, for example to use them in other programs. To prepare a BED track:

```bash
sort -k1,1 -k2,2n genes.bed | bgzip > genes.bed.gz && tabix -p bed genes.bed.gz
```

For GFF3 and GTF, sort by columns 1 and 4 and use `tabix -p gff`:

```bash
sort -k1,1 -k4,4n genes.gtf | bgzip > genes.gtf.gz && tabix -p gff genes.gtf.gz
```

genie shows features when the view is 10 Mb or less. Wider views show the density of the features: bigBed files from their zoom levels, other files as the number of features per Mb. For a file with a tabix or CSI index, genie estimates the density from the index, without reading the features. This needs an index from htslib (tabix, bcftools or samtools), which counts the records of each contig.

To prepare a BAM track:

```bash
samtools sort -o reads.bam reads.sam && samtools index reads.bam
```

A CRAM file keeps the bases of a read as differences from a reference. genie uses the reference that is open, so open the genome or the FASTA file that made the CRAM file. Each part of a CRAM file has the MD5 of its reference bases. If the open reference is different there, the track says "The reference is not the reference of the CRAM file here." and does not show wrong bases. CRAM files that hold their own reference, or that do not need one, open with any reference. genie reads CRAM 3.0 and 3.1 files with gzip, rANS and rANS Nx16 compression. This is what most archives and samtools make by default. genie does not read the arithmetic coder, fqzcomp and the name tokenizer of CRAM 3.1, and bzip2 and lzma blocks. The `small` and `archive` profiles of samtools use them.

genie shows reads when the view is 30 kb or less. It hides QC-fail and secondary reads. By default, it also hides duplicate reads and soft clips, and it keeps 100 reads for each 50 bp.

To prepare a large VCF track:

```bash
bgzip calls.vcf && tabix -p vcf calls.vcf.gz
```

genie shows variants when the view is 1 Mb or less. Wider views show the number of variants per Mb, estimated from the index as for feature files. Put the pointer on the bars to see the density, for example "About 520 variants per Mb".

To prepare a large bedGraph track:

```bash
sort -k1,1 -k2,2n signal.bedGraph | bgzip > signal.bedGraph.gz && tabix -p bed signal.bedGraph.gz
```

genie shows bigWig tracks and plain bedGraph tracks at all zoom levels. It shows a bedGraph track that has a tabix index when the view is 10 Mb or less. For the best speed, convert bedGraph to bigWig with bedGraphToBigWig.

A bedMethyl file from `modkit pileup` is a BED file. To prepare a large bedMethyl track:

```bash
sort -k1,1 -k2,2n calls.bed | bgzip > calls.bedMethyl.gz && tabix -p bed calls.bedMethyl.gz
```

genie shows a bedMethyl track that has a tabix index when the view is 10 Mb or less.

### GenBank and EMBL files

A GenBank or EMBL file has the sequence and the features of one or more records, for example a plasmid from SnapGene or Addgene, or a virus or a bacterium from NCBI.

- When you open the file, its sequence becomes the reference and its features a track. Each record is a contig. If the open reference already has the same records, with the same names and lengths, only the track opens. So you can open the GenBank features of a genome that you opened as a FASTA file.
- A record has the name of its accession and version, for example `NC_045512.2`, as NCBI names the sequence in its FASTA files. A record without an accession, as many plasmid files are, has its LOCUS name, for example `pUC19`.
- A gene shows as its transcripts: each mRNA, ncRNA or other RNA with its exons, and with the coding part of the CDS that has the same introns. A CDS without a transcript, as in bacteria and viruses, shows on its own. The gene itself does not show again. Other features, such as promoters, origins and repeats, show on their own. The source feature does not show.
- A feature has the name of its gene, its locus tag, its label or its product. A transcript or a CDS also keeps its ID (transcript_id or protein_id), and you can type a name or an ID in the locus box. The card shows the type of the feature and its qualifiers, without the protein sequence.
- Features use the colors that SnapGene, ApE, Benchling and Artemis write in the file. Right-click the track and turn off **Use file colors** to use the track color.
- A feature that crosses the origin of a circular record shows as two parts, one at each end.
- genie keeps the bases of the file in memory, so the limit is 512 MB.

### PAF alignments

A PAF file from minimap2, wfmash or another aligner shows each alignment on its target sequence, as a feature with the name of the query. So make the PAF file with the reference in view as the target, for example `minimap2 -cx asm5 genome.fa other.fa > other.paf`.

- If a line has a CIGAR (the `cg` tag, from `minimap2 -c`), each deletion of 50 bases or more from the target splits the alignment into blocks with a thin line between them, as the exons of a transcript.
- The card shows the place on the query, for example `contig_7:1001-4100`, the identity (matches / alignment block length), the matches, the block length, MAPQ and the short tags, such as the type (primary, secondary or inversion), the edits (NM) and the divergence (dv). Filters work on these fields, for example **Identity (%)** at least 95 or **Type** primary.
- Type a query name in the locus box to go to its alignment.
- A plain PAF file up to 50 MB opens directly. genie sorts and indexes larger files in its cache. To prepare a PAF file yourself:

```bash
sort -k6,6 -k8,8n other.paf | bgzip > other.paf.gz && tabix -0 -s6 -b8 -e9 other.paf.gz
```

## Files on the web

genie can open reference sequences and tracks from a web server. To open a file from the web:

1. Push Ctrl+U (Cmd+U on macOS) or click the link button.
2. Type the address of the file. The address must start with `https://`, `http://`, `s3://` or `gs://`.
3. Push Enter.

You can also give addresses as arguments:

```bash
cargo run -p genie -- https://example.org/data/genome.2bit https://example.org/data/reads.bam
```

- genie reads only the parts of a file that the view needs. The server must send parts of files (HTTP range requests). If the server sends only full files, genie opens only files of 4 MB or less.
- genie looks for the index on the same server. The address of an index is the address of the file with the index extension, for example `reads.bam.bai`.
- A 2bit file on the web opens with the contig sizes of the `.chrom.sizes` file beside it, when there is one, as at UCSC (`hg38.chrom.sizes` beside `hg38.2bit`). Else genie reads the length of each contig from the 2bit file, and a genome with many contigs takes many requests. Sessions keep the sizes file.
- If a track file on the web has no index, genie downloads the file into its cache folder and prepares it there, as for files on your computer. The limit is 1 GB. If you stop a download, it continues from where it stopped the next time that you open the file. genie does not make indexes for FASTA files from the web.
- genie reads plain BED, GFF3, GTF, VCF, bedGraph and bedMethyl files up to 50 MB completely.
- For files that genie reads from the web in parts, the locus box finds names only in bigBed files that have a name index.
- genie keeps up to 64 MB of data from the web in memory. It writes a file from the web to disk only when it must make an index for the file.
- genie sends at most 4 requests at one time to one server, and reads near parts of a file in one request. When a server is busy (HTTP 429 or 503), genie waits as the server asks and tries again, three times at most. If the server asks for a wait of more than 30 seconds, genie stops and tells you.
- genie opens files in public buckets of Amazon S3 and Google Cloud Storage, for example `s3://1000genomes/phase3/data/HG00096/alignment/HG00096.chrom20.ILLUMINA.bwa.GBR.low_coverage.20120522.bam` or `gs://gcp-public-data--gnomad/release/4.1/vcf/genomes/gnomad.genomes.v4.1.sites.chr21.vcf.bgz`. genie reads them through the HTTPS addresses of the buckets. It does not sign requests, so it cannot open private buckets or Requester Pays buckets.
- genie uses the certificates of the operating system for HTTPS. A connection to an `http://` address is not secure. genie does not send user names, passwords or cookies.
- Sessions and the recent list keep the addresses of files from the web. genie cannot open a session file from the web.

## Navigation

- Drag the tracks to move along the sequence. Turn the wheel with Ctrl (Cmd on macOS) to zoom. Push the arrow keys to move and + or - to zoom. The buttons right of the locus box zoom out, zoom in, show the whole chromosome and show the [whole genome](#whole-genome).
- Click the genie logo for a menu with the main actions and their keys. Push Ctrl+K (Cmd+K on macOS) to find an action by its words, for example "group phase", then push Enter. Push F1 or click the ? button to see all keys and mouse actions.
- In a narrow window, the title bar hides the buttons that the genie menu also has: save, export, help, settings and the whole genome. Then the other buttons and the locus box stay in the window.
- Drag on the ruler to zoom to that range. Push Escape to stop the drag.
- To go back to an earlier view, push Alt+Left (Cmd+[ on macOS), push the back button of the mouse or click the back arrow left of the locus box. Alt+Right (Cmd+] on macOS) goes forward again. genie keeps the last 100 jumps: the locus box, name search, ruler drags, region and feature jumps and changes of contig. Moves and zooms do not add a step.
- Push Ctrl+F or Ctrl+B (Cmd on macOS) to go to the next or the previous feature or variant of the track under the pointer. The search continues on the next contigs. The track menu has the same items.
- genie sorts the contigs. First are the numbered chromosomes in number order (1, 2 … 22, also arms such as 2L and 2R, and Roman numbers such as I to XVI). Then come X, Y, W and Z, and then M. The other contigs follow in the order of the file. The region list and the feature search use this order.

## Find a sequence

- Push Ctrl+Shift+F (Cmd+Shift+F on macOS), or click **Find a sequence…** in the logo menu or the palette. A find bar opens under the ruler.
- Type the bases, for example a primer. You can use the IUPAC codes, for example N, R and Y. genie ignores spaces, digits and the marks in `5'-…-3'`. The sequence can have 2 to 200 bases.
- genie finds the bases on the two strands. A lane under the find bar shows a bar for each match in the view, with arrows for its strand. A sequence that is the same on the two strands, such as `GAATTC`, shows one bar without arrows.
- Click 1, 2 or 3 next to **Mismatches** to also find near matches. Near matches have a light color, and when you zoom in, the wrong bases have a vermillion mark. Each mismatch needs 4 bases in the sequence, for example 8 bases for 2 mismatches. Unknown bases (N) in the reference do not match.
- Push Enter to go to the next match, and Shift+Enter to go to the previous match. If there is no match near the view, genie reads 20 Mb of the reference each time that you push Enter. At the end of a chromosome, it continues on the next chromosome.
- Put the pointer on a match to see its locus, its strand, its mismatches and its bases as they read on its strand. Wrong bases are lower case. Click to keep the card open.
- genie finds sequences when the view is 10 Mb or less, and shows up to 100,000 matches. Push Escape or click × to close the find bar.

### Primer pairs

- Click **Primers** in the find bar to find the PCR products of a primer pair, as an in-silico PCR. Type the forward primer and the reverse primer, each from its 5′ end to its 3′ end. Each primer needs 12 bases or more.
- A product starts at the forward primer on one strand and ends at the reverse primer on the other strand. genie finds the products on the two strands of the reference, up to 4,000 bp long. In the lane, a product is a line with a block for each primer and arrows for its strand.
- **Mismatches** allows 1, 2 or 3 wrong bases in each primer. The 5 bases at the 3′ end of each primer must match, because the polymerase extends from there.
- Push Enter and Shift+Enter to go to the next and the previous product. Put the pointer on a product to see its locus, its length, its strand, its mismatches and its primers.
- Click **Bases** to find one sequence again.

### Primer design

genie designs PCR primers around a target, as Primer3 does with its default settings, and checks that each pair makes only its own product.

- Click **Primers** in the open card of a variant, or right-click the ruler or a region of the region list and click **Design primers…**. The target is the variant, the view or the region. The palette also has **Design primers for the view…**.
- A dialog shows the 5 best pairs. Click **PCR 150–500**, **Sanger 400–800** or **qPCR 70–150**, or type the shortest and the longest product, then click **Design**.
- Each primer has 18 to 25 bases, a melting temperature (Tm) of 57 to 63 °C, 20 to 80 % GC, no run of more than 4 of one base, and 1 to 3 G or C in the 5 bases at its 3′ end. genie finds the Tm by the nearest-neighbor values of SantaLucia (1998), for 50 mM of monovalent ions, 1.5 mM of Mg²⁺, 0.6 mM of dNTPs and 50 nM of each primer, as Primer3. The two primers of a pair differ by 5 °C at most, and they do not pair much with themselves or with each other. The best pairs have a Tm near 60 °C and 20 bases.
- Primers stay off the variants of the variant tracks that show. A track with a filter keeps primers off only the variants that the filter keeps, for example the variants with a frequency of 1 % or more.
- genie then checks where each primer binds: where the 8 bases at its 3′ end match and its other bases have 2 mismatches at most, on the two strands. A pair that also makes other products of 4 kb or less drops out. For a reference on this computer, genie checks all chromosomes, which takes some seconds to a minute for a human genome. For a reference on the web, it checks 1 Mb around the target. If each pair makes other products, the dialog shows the best ones with a note.
- Each pair shows its product, its products in the genome, and each primer from its 5′ end with its length, its Tm and its GC content. Click **Show** to go to the product: the find bar shows it as the PCR product of the two primers. Click **Copy** to copy the two primers, each with the locus of the product.

### BLAT

The BLAT server of UCSC finds where a sequence aligns in the whole genome, also with gaps and introns. genie can send a sequence to it when the open genome is a genome of UCSC, such as hg38 or mm39.

- Click **BLAT a sequence…** in the palette, type or paste the bases (plain or FASTA, 20 to 8,000 bases), then push Enter or click **BLAT**.
- To BLAT a read, click the read to keep its card open, then click **BLAT** in the card. If the read has a soft clip of 20 bases or more, **Use the clipped part** puts the longest clip in the box, for example to find where the clipped part of a split read comes from.
- UCSC takes BLAT searches from programs only with the API key of a person. To make your key, log in to your account at genome.ucsc.edu, open the Hub Development page and make a key in its API key part. Paste the key in **UCSC API key** in the dialog. genie keeps the key in its settings file on this computer and does not show it in messages.
- Without a key, **Search on the UCSC page** opens the same search in your web browser. genie also opens PSL files that you save from UCSC.
- genie sends only the bases and your key to UCSC. UCSC asks for at most one search each 15 seconds, so genie waits that long between two searches.
- The dialog lists the alignments, the best score first, with the locus, the strand, the identity, the score and the length. Click an alignment to go there.
- genie keeps the alignments as a PSL file in its cache folder and opens it as a track, named "BLAT" and the name of the sequence. The track shows each alignment with its blocks, and Ctrl+F and Ctrl+B go to the next and the previous alignment. The card of an alignment shows the place on the query, the identity, the score, the matches, the mismatches and the gaps.
- genie opens other PSL files too, for example from BLAT on the command line. Index a large PSL file without its header with `tabix -0 -s14 -b16 -e17`, or let genie prepare it in its cache.

## Several loci

- Type two to four loci in the locus box, with a space between them, for example `BRCA1 BRCA2`. genie shows the loci side by side in panels of equal width.
- The top of each panel shows its locus and its ruler. Click the locus to show only that panel. Click × to close the panel. To show one panel again, type one locus.
- Drag, turn the wheel or drag on the ruler in a panel to move or zoom that panel. The arrow keys and + and - move and zoom all panels.
- Put the pointer on a feature, a read or a variant in a panel to see its data.
- Back, forward, region jumps and feature jumps move the first panel.
- Sessions, Ctrl+C (Cmd+C on macOS) and the `goto` command keep all loci. Images show all panels.

## Whole genome

- Type `All` (or `genome`) in the locus box, or click the button with four bars right of the zoom buttons, to see all main chromosomes side by side. The genie menu and the palette also have **Show the whole genome**.
- Each chromosome is as wide as its share of the genome, and at least 3 pixels wide. The ruler shows the names of the chromosomes that have space, without "chr". genie does not show alt, random, unplaced and patch contigs (names with `_`). If a reference has more than 50 contigs, genie shows the 50 longest.
- Tracks show what they show for a whole chromosome: signal tracks show their bars, copy number tracks show their rows, and feature and variant tracks show their density. All chromosomes use one scale, and the range shows once at the right of the track. Read tracks and other tracks that need a closer view show one hint.
- Click a chromosome on the ruler or on a track to show it. Drag across a part of a chromosome to show that region. Put the pointer on the data to see its card.
- Back and forward return to the whole-genome view. Sessions keep it.

## Regions

- Click the bookmark button to show the region list. Push Ctrl+D (Cmd+D on macOS) to add the view to the list, or hold Shift and drag on the ruler to add a range.
- Click a region to go to it. Double-click a region to change its name, then push Enter.
- Import BED and Export BED read and write the list as a BED file. genie reads plain BED files up to 50 MB. It does not import regions on contigs that are not in the reference.
- **Lift…** moves the regions from a different assembly to the assembly in view with a chain file (see [Liftover](#liftover)).
- Regions show as bands under the ruler. Put the pointer on a band to see its name, and click the band to go to the region.

## Copy

- Push Ctrl+C (Cmd+C on macOS) to copy the locus of the view, for example `chr1:1,001-2,000`. If the view has several loci, genie copies all of them, with a space between them. If you clicked a card to keep it open, Ctrl+C copies the text of the card.
- Right-click the ruler or a region in the region list, then click **Copy locus** or **Copy sequence**.
- **Copy sequence** copies the bases in FASTA format, with 60 bases on each line. The header is the locus, for example `>chr1:1001-2000`. If the sequence track shows the reverse strand, genie copies the reverse complement and adds `(-)` to the header. genie copies up to 1 Mb.

## Tracks

- Put the pointer on a feature, a read or a variant to see its data. Click to keep the card open.
- A card also shows the other fields of the file: bigBed fields with the descriptions from the file, GFF3 and GTF attributes, and the BED columns after column 12 (Column 13 and so on). A variant card shows the INFO entries with the descriptions from the `##INFO` lines of the header. If two descriptions give the same short label, the card shows the INFO IDs, for example `AF_EUR` and `AF_EUR_unrel`. genie does not show empty fields, fields with the value `none` or `.`, and bigBed fields that UCSC hides (names that start with `_`).
- In a bigBed card, each item of a list keeps its place, and an item without a value (empty, `-inf` or `nan`) shows as –. For example, the alleles and the frequencies of the projects in dbSNP agree item by item. A Sequence Ontology number shows its term, for example `5_prime_UTR_variant (SO:0001623)`. genie does not show 0, which is no term.
- A card shows up to 6 fields. Click to keep the card open and see all fields. If the card is taller than the window, scroll it. Ctrl+C (Cmd+C on macOS) copies the card with all fields.
- genie keeps the first 400 characters of the INFO column of each variant, without the annotations of VEP, SnpEff and bcftools (see below). If it cuts the INFO column, the card shows **More INFO**.
- A variant card shows QUAL and FILTER as the fields Quality and Filter.
- A variant card also shows the change of the variant on the transcripts of the open feature tracks, in HGVS notation, for example "BRCA1 p.Arg1699Trp · c.5095C>T". genie uses the transcripts in view that have a strand and a coding part, for example from BED12, bigBed, GFF3 and GTF files, and gets the bases from the reference. The card shows up to 3 transcripts, with the protein changes first, and the number of the other transcripts. "p.Leu12=" keeps the amino acid, "p.Arg37Ter" makes a stop codon and "p.Lys45fs" moves the reading frame. A variant in the 2 bases next to an exon shows "splice site". A variant out of the coding sequence shows its place and its c. position: 5′ UTR (for example "c.-14G>A"), 3′ UTR ("c.*32C>T") or intron ("c.92+5G>A"). A deletion of whole exons shows "exon deletion", and a deletion of the whole transcript shows "transcript deletion". A change of the stop codon shows the extension, for example "p.Ter148GluextTer?". An allele with unknown bases (N) shows "p.?". As HGVS asks, genie moves an insertion or a deletion in a repeat to the 3′ end of the transcript, and an insertion of the bases before it is a duplication, for example "c.417dup" or "p.Lys2dup". Each alternate allele of a site has its own lines. genie does not use a transcript if the length of its coding part is not a multiple of 3.
- If the VCF file has annotations from Ensembl VEP (`CSQ`), SnpEff (`ANN`) or bcftools csq (`BCSQ`), the variant card shows the 3 most severe annotations, one line for each, for example "BRCA1 · missense_variant · MODERATE · ENST00000357654.9:c.5095C>T · p.Arg1699Trp", and the number of the other annotations. For the same impact, the transcripts that VEP marks as canonical, MANE Select or picked come first. bcftools csq does not give the impact, so genie gets it from the consequence, as VEP does.
- An open card of a variant on hg38 or hg19 can show its allele frequency in gnomAD: v4.1 for hg38 and v2.1.1 for hg19, with the exomes and the genomes together. genie knows a human reference by its genome or by the length of chromosome 1. To start, click **Ask gnomAD** in the open card, or turn on **gnomAD** in Settings. genie then sends the place and the alleles of each variant whose card you keep open to the gnomAD API (gnomad.broadinstitute.org), and of no other variant. The card shows the frequency, the alleles and the homozygous samples, for example "gnomAD v4.1: 1.23 % · 12 of 974 alleles · 1 homozygote", and the frequency of each genetic ancestry group. A site with more than one alternate allele shows a line for each of its first 3 alleles, without the groups. genie writes an allele as gnomAD does, without the bases at its ends that the two alleles share, so an allele that genie cannot find can have a different form in gnomAD. gnomAD answers at most about 10 questions a minute, so genie asks at most 8 a minute and keeps each answer until it closes. Turn off **gnomAD** in Settings to stop.
- The fields **Consequence**, **Impact**, **Gene** and **Biotype** list the values of all annotations of the variant. Use them to filter the track, for example to keep only the variants with a HIGH impact. The other columns of the first annotation are fields with the name of the INFO field before them, for example `CSQ SIFT`. genie finds annotations in each INFO field whose description names its columns, as VEP, SnpEff and bcftools write them.
- To filter a track, click a value in a card that you keep open. Then click **Keep only** or **Hide** the value. For a number, the menu has a box with the value: keep it or type a different number, then click **At least** or **At most**. Each rule hides more features or variants. A value with commas passes when one of its items passes, for example the tags of a GENCODE transcript or the frequencies of a dbSNP variant.
- A filtered track shows the number of features or variants in view that it keeps, for example "12 of 586", at the top right. Click the number, or right-click the track, to see the rules. Click × to remove a rule.
- Click **Label with this field** in the same menu to name the features with that field, for example the gene type. **Label with name** in the track menu goes back.
- Filters and labels apply when the track shows features or variants. A filtered bigBed track wider than 10 Mb shows the density of the features that the filter keeps, if their data in view is 16 MB or less. If there is more data, the track shows the density of all features, and the count asks you to zoom in. **Next feature** and **Previous feature** skip the features and variants that the filter hides. If you type a name that a filter hides, genie goes to it and a note says so, for example "TP53 is hidden by the filter of GENCODE V48." Sessions and images keep the filters. In a session file, a rule of a VCF track can also name an INFO key, for example `least = EUR_AF: 0.05`: the rule takes the label of the field of that key.
- Type a feature name or a variant ID in the locus box to go to it. The search ignores letter case. genie finds variant IDs in VCF files up to 50 MB. It also finds names in bigBed files that have a name index (`bedToBigBed -extraIndex=name`), and in other bigBed files up to 50 MB.
- BED and bigBed features use the colors of the file (itemRgb). Right-click the track and turn off "Use file colors" to use the track color.
- In a bigBed file of gene models (bigGenePred), each transcript shows its gene symbol. genie uses the field that the file describes as the gene symbol. If there is no such field, it uses `geneName2`, then `name2`. A track from the [UCSC track list](#ucsc-tracks) can set the field. For example, mm10 UCSC Genes uses `geneName`.
- In a variant track, the color of a site shows the change: the color of the new base for an SNV, purple for an insertion and dark grey for a deletion. A site that does not pass FILTER has a light color.
- Structural variants from callers such as Sniffles2, Manta, DELLY, pbsv and cuteSV show over their full length: a deletion in dark grey, a duplication in vermillion, an inversion in green and an insertion in purple. genie finds them by their SVTYPE, by a symbolic allele such as `<DEL>` or `<DUP:TANDEM>`, or by a breakend such as `G]chr17:198982]`. The card says the type and the length, for example "Deletion · 2,345 bp", and "imprecise" for an IMPRECISE call. A complex rearrangement, such as a CPX record of gnomAD-SV, also shows over its full length, and its card says "Complex rearrangement".
- Thin lines with ends show the confidence intervals of the start and the end (CIPOS and CIEND).
- A lane above the sites shows each breakend (BND) as an arc to its other end. Two breakends that join each other give one arc. A breakend to another chromosome shows as a stub, and its card names the other end, for example "Breakend · other end chr2:321,682". genie finds the other end in the alternate allele, or in CHR2 with POS2, END2 or END, as gnomAD-SV and other callers write it. A breakend takes one base, also when its END is on the other side.
- Under the sites, each sample has a row: a light color for heterozygous, the full color for homozygous alternate and grey for homozygous reference. A sample with no call has no mark. If there are more than 100 samples, the rows are thin. If there are more than 300 samples, each row shows a group of samples with the strongest call of the group (homozygous alternate, then heterozygous, then homozygous reference), and the card names the samples of the row.
- A signal track has a bar for each pixel. The bar goes to the mean value, and a light color goes to the maximum value. Values below zero go down from a zero line. The range of the values in view is at the top right.
- Right-click a signal track to show a line instead of bars. Turn on "Shared scale" on two or more signal tracks to give them the same range.
- To fix the range of a signal track, right-click it, type the minimum and the maximum, then push Enter. The fields show the range in view. **Auto range** goes back to the range of the data in view. A track with a fixed range does not use "Shared scale".
- To show two or more signal tracks in one lane, right-click a signal track, then click a track under **Overlay on**. The lane shows each signal as a line in the color of its track, with one range for all. If two tracks have the same color, genie gives the added track a different color. To show a track in its own lane again, right-click the lane and click the track under **Separate**.
- To compare a signal track with another signal track, for example ChIP with its input, right-click the track and click the other track under **Compare with**. The lane then shows the log2 ratio of the two values at each pixel, with 1 added to each value, so that a value of 0 does not give an infinite ratio. Right-click the lane to show the **Ratio** or the **Difference** instead, or click **Stop comparing**. A chip at the right of the lane names the comparison, and the card shows its value. The lane shows nothing where the other track has no value. If you remove the other track, the comparison stops. Sessions keep the comparisons.
- Double-click a track name to change it, then push Enter. Push Escape to stop. If you delete the name, the track shows its file name again. The track menu also has **Rename**.
- Right-click a track to set its rows, its color or its place. When the pointer is on a track, a ⋯ button next to its name opens the same menu and a × button removes the track.
- A feature track shows at most 100 rows. An expanded track that needs more rows shows squished, and a line under the rows counts the rows that do not show, for example "242 more rows · zoom in or filter". Zoom in, or filter the track, to see them.
- Click **Track settings…** at the top of the track menu to see all settings of the track in one dialog: name, color, rows, height, and the settings of its kind. Each change shows at once.
- Right-click a read track to show duplicate reads and soft clips, or to keep all reads.
- genie colors unusual read pairs: vermillion for an insert size that is larger than expected, blue for a smaller one, green for two mates on the same strand, purple for mates that point away from each other, and orange for a mate on a different chromosome. The expected insert sizes come from the loaded pairs. Right-click a read track to turn these colors off.
- Right-click a read track and click **View as pairs** to put the two mates of each pair on one row, with a thin line between them. A pair takes the space from the start of one mate to the end of the other, so pairs with an unusual insert size are easy to see. A mate whose other mate is not in the loaded region stays alone, and secondary and supplementary reads do not join a pair.
- An insertion in a read has a purple mark when you zoom in to single bases. An insertion of 10 bases or more, as long reads often have, has a purple box with its length in every view, and a mark in squished and collapsed rows. Put the pointer on the mark or the box: the read card shows the length and the place of the insertion, and the field **Inserted bases** holds its bases. Ctrl+C (Cmd+C on macOS) copies the card with all the bases.
- Right-click a read track to sort the rows by the base at that position, or to group the reads by phase (HP tag) or by strand.
- The card of a paired read shows the mate position, the insert size, the pair orientation and the HP and RG tags.
- To see a mate that is far away or on another chromosome, click the read to keep its card open, then click **View mate**. genie shows two panels side by side, each as wide as the view: the read in the middle of the first, the mate in the middle of the second.
- A read card also lists MAPQ, the insert size of a pair and the small tags of the read as fields, for example NM, AS or CB. In a card that you keep open, click a field to keep only or hide the reads with that value, or to keep the reads with at least or at most a number. genie loads the reads again with the filter. For a tag, the menu also has **Group by this tag** and **Color by this tag**. Groups show numbers in order, then text values, then the reads without the tag. A track shows at most 30 groups: the smallest ones go together as "Other values". Color by a tag gives the 5 most common values in the loaded reads the 5 track colors. The other reads are grey. Right-click the track and click **Group by** or **Color by** with the tag name to stop. CRAM files from samtools usually leave out the NM and MD tags, so their reads do not show them.
- Right-click a read track to show secondary reads, supplementary reads or reads that failed the quality checks of the sequencer, and to hide the reads under MAPQ 1, 10, 20 or 30 (**MAPQ ≥**). Sessions keep these settings.
- A read with MAPQ 0 is placed in more than one spot of the genome. genie shows it in a light color, and its card says "placed in more than one spot".
- A split read has other parts in the genome (SA tag). A dark mark shows the clipped end of the read. The card lists the other parts, for example "chr2:1,234,000 + · MAPQ 60". In a card that you keep open, click a part to go to it.
- A read that spans an intron (N in the CIGAR) shows a thin line over the intron. Above the reads, an arc joins the two ends of each splice junction. A higher and thicker arc has more reads. The color of the arc shows the strand: blue for forward, vermillion for reverse and grey if the strand is not known. The strand comes from the XS or ts tag of the reads. If the reads have no such tag, it comes from the bases at the two ends of the intron: GT-AG, GC-AG or AT-AC on the forward strand, or their reverse complements. If there is space, the arc shows its number of reads. The count includes all reads, also the reads that down-sampling hides. Put the pointer on an arc to see its position, reads, strand and intron length. A collapsed read track shows the coverage and the arcs.
- Long reads can have base modifications (MM and ML tags), for example 5mC from nanopore or PacBio. Right-click a read track and turn on **Color by modification**. The reads become grey, and each call has a color: vermillion for modified, blue for not modified, and a light color for a probability near 50%. Mismatches stay on top. When you zoom out, a pixel shows the mean probability of its calls. The card of a read counts the calls for each modification, for example "Modified 5mC 34 of 120" (calls with a probability of 50% or more).
- When **Color by modification** is on, a lane above the reads shows the share of modified reads at each site. A vermillion bar goes up to the share of modified calls, and grey fills the rest. If you group the reads by phase or by strand, each group has a lane with the group name. At a CpG, genie adds the calls of the two strands together, at the C of the forward strand. Put the pointer on a bar to see the counts, for example "Modified 12 of 20 reads · 60 %", and the counts of the other modifications at the site. The lane shows 5mC, or the modification with the most calls if the reads have no 5mC. To show a different modification, right-click the track and click it, for example **Show 5hmC**.
- When the reads are in groups, for example by phase, right-click the track and turn on **Methylation difference**. A lane under the methylation lanes compares the first two groups, for example haplotype 1 against haplotype 2, at each site. A bar goes up when the first group has more modified reads and down when the second group has more, up to 100 percentage points. A dot marks the sites where Fisher's exact test on the read counts gives p < 0.05. Put the pointer on a bar to see the counts of each group, the difference and p. genie does not correct p for the number of sites.
- genie opens bedMethyl files from `modkit pileup` as signal tracks. genie knows a bedMethyl file by its name (`.bedMethyl`, also compressed) or by its columns, also if the name ends with `.bed`. The bars show the percent of modified reads, from 0 to 100. Right-click the track to select the modification, for example **Show 5hmC**. **Join CpG strands** adds the counts of the two strands of each CpG together, at the C of the forward strand. genie joins two sites only if the file has a site on each strand. Turn it off to see each strand. Zoom in until a pixel shows less than 8 bp: then the card of a site shows its counts, for example "Modified 12 of 20 reads · 60 %".
- To compare two groups of bedMethyl files, for example tumor samples with normal samples, put each group in a folder of the tracks panel. Right-click a track of the first folder and click a track of the second folder under **Compare with**. Then right-click the lane and click **Methylation difference**. At each site, genie adds the counts of the bedMethyl tracks of each folder. The lane shows the difference as for the groups of a read track: a bar up when the first folder has more modified reads, a bar down when the second folder has more, and a dot where Fisher's exact test gives p < 0.05. Put the pointer on a bar to see the counts of each folder, the difference and p. A hidden track is not loaded, so its counts are not in the sum. A track that has no folder, or that is in the same folder as the other track, is a group alone.
- To list the DMRs (differentially methylated regions) of a methylation difference, right-click the track and click **DMR table**. The palette also has **Show the DMR table**. A table under the tracks lists the DMRs of the sites in view: runs of at least 3 CpGs in a row, each with p < 0.05 and a difference of 10 percentage points or more in the same direction, with at most 500 bp between two CpGs. One CpG that does not keep to these rules does not end a run, but two in a row do. Such a CpG is in the length of the DMR, but not in its CpGs or its mean difference. Each row shows the place, the CpGs, the length, the mean difference (the first group minus the second group) and the smallest p. Click **5** or **10** at **CpGs at least** to keep only the longer DMRs. Click a row to go to its DMR. **Export BED** saves the DMRs as a BED file, with the mean difference as the name and the number of CpGs as the score. genie does not correct p for the number of sites.
- An interaction track shows links between two regions, for example chromatin loops from Hi-C or ChIA-PET. genie opens bigInteract files (bigBed files with the UCSC interact fields, also with the older names `region1` and `region2` of the ends), BEDPE files, and the loop lists of Juicer and HiCCUPS. In a loop list, the header line gives the names of all fields, and the `color` column gives the color of each arc. Each link is an arc from the middle of one end to the middle of the other, with a small block at each end. A longer link rises higher. A link with a larger value (the `value` field of bigInteract, the score of BEDPE) has a thicker line. An arc with an end outside the view goes past the edge. A link to another chromosome shows as a short stub at its end in view. The arcs use the colors of the file; right-click the track and turn off **Use file colors** to use the track color. Put the pointer on an arc, an end or a stub to see the two ends, the distance and the other fields. A BEDPE header line gives the names of the fields. Zoom in to 50 Mb or less to see the links of a bigInteract file.
- A junction track shows the splice junctions of a STAR `SJ.out.tab` file or a junction BED file as arcs, as the junction lane of a read track does. An arc goes from the start to the end of the intron. A junction with more reads has a higher and thicker arc, with the number of its reads over it: for STAR, the uniquely mapped reads. The color shows the strand: blue for forward, vermillion for reverse, and grey when the file does not give it. Put the pointer on an arc to see the intron, the reads and the fields of the file, for STAR the multi-mapping reads, the splice motif (for example GT/AG), if the junction is annotated, and the largest overhang. In a card that you keep open, click a field to keep or hide junctions, for example the junctions with at least 10 unique reads. When more than 5,000 junctions are in view, the track shows the 5,000 with the most reads. Drag the bottom edge of the track to make it taller.
- A Hi-C track shows the contacts of a `.hic` file of Juicer (versions 6 to 9) as a triangle over the diagonal: contacts between near bins are at the bottom, and contacts between bins farther apart are higher. Each contact is a block of one bin, in rows that stand for the rotated matrix. The color goes from light grey to vermillion; the full color starts at the count that is higher than 95% of the counts in view that are 2 bins or more from the diagonal. The diagonal and its next bins, which have much higher counts, get the full color. If the middle of those counts would get less than a fifth of the full color, the colors follow a curve that gives it a fifth. This keeps a view of a whole chromosome bright, where the contacts of near bins are much higher than the others. The size of the bins follows the zoom: genie uses the smallest bin size of the file that gives blocks of 2 to 8 pixels. genie reads only the blocks of the matrix that the view needs, also from the web.
- Right-click a Hi-C track to select the normalization of the file, for example **KR normalization** or **SCALE normalization**, or **No normalization** for the counts. Without a choice, genie uses SCALE, KR, VC_SQRT or VC, in this order, if the file has it. If the file has no vector of a normalization for a chromosome at a bin size, the map shows the counts. **Observed/expected** divides each value by the expected value at its distance: blue below 1 and vermillion above 1, with the full colors at 1/4 and 4. To set the colors of the counts, type a count in **Full color at** and push Enter: the colors then follow a straight line to that count. **Auto range** goes back. Sessions keep the count. Put the pointer on a block to see its two bins, their distance, the value and the normalization. Drag the bottom edge of the track to make the triangle taller.
- genie opens cooler files (`.cool`, and `.mcool` with several bin sizes) as Hi-C tracks too. It reads the chromosomes, the bins and the pixels of the HDF5 file, and only the pixels of the view. The normalizations are the columns of balancing weights of the bins, such as **weight normalization** from `cooler balance`: genie multiplies each count by the weights of its two bins. Without a choice, genie uses `weight` if the file has it. A cooler file has no expected values, so **Observed/expected** divides each value by the mean value of the contacts in view at its distance. genie reads the HDF5 files that cooler writes: datasets that are whole or in chunks, with the deflate, shuffle and Fletcher-32 filters.
- For a virtual 4C, right-click a Hi-C track at a place and click **Virtual 4C from here**. A lane under the map shows the contacts of the bin at that place with each bin in view, as bars, and a line marks the place. The lane follows the normalization and **Observed/expected**, and the bin size follows the zoom. Put the pointer on the lane to see the value of a bin. The viewpoint must be in the loaded part of the map. Click **Stop virtual 4C** to remove the lane. Sessions keep the viewpoint.
- genie opens a table compressed with bgzip that has a tabix index as a table track, for example the CADD scores. The index gives the columns of the chromosome, the start and the end. The last header line (`#Chrom Pos …`) gives the names of the columns. A table track shows a column of numbers as a signal: at first the last column of numbers, for example PHRED. Right-click the track to show a different column, for example **Show RawScore**, or **Show rows** to see each row as a feature with its columns as fields, named by its first column of text. Zoom in to 10 Mb or less to see the data. Sessions keep the choice. A `.tsv` or `.txt` file without a tabix index stays a sample table.
- A GWAS track shows each variant of a file of summary statistics as a dot at the height of its -log10 p (a Manhattan plot). A grey line shows the genome-wide significance of 5×10⁻⁸, and the dots over it are vermillion. The lane goes up to the highest value in view. genie knows the file by its header line, which names the chromosome, the position and the p-value or the -log10 p, for example `chromosome base_pair_location … p_value` (GWAS Catalog), `#CHROM POS … P` (PLINK 2) or `CHROM GENPOS … LOG10P` (REGENIE). Put the pointer on a dot to see its row. A file with a tabix index opens at once, and genie reads only the part in view; for a file on the web, zoom in to 50 Mb or less. genie reads all of a file without an index, up to 1 GB. In the whole-genome view, every second chromosome has a lighter color.
- A copy number track shows the segments of each sample in a row: blue for a loss, vermillion for a gain, and light grey near 0. The full colors start at a log2 value of -1.5 and +1.5. To use a different range, right-click the track, type the minimum and the maximum, then push Enter. **Default range (±1.5)** goes back. genie finds the columns by the header line of the file: the sample, the chromosome, the start, the end, the number of markers and the value (for example `seg.mean`, `log2` or `MEAN_LOG2_COPY_RATIO`). If the header names no value column, genie uses the last column. A CNVkit file gets its sample name from the file name, and a GATK file from its `@RG` line. Up to 20 samples show expanded, with their names on the right. With more samples, the rows get thinner to fit, down to 1 px. Put the pointer on a segment to see its sample, its length, its markers and its value.
- Right-click the sequence to show the reverse strand and 3 or 6 reading frames.
- Drag a track name to move the track.
- Drag the bottom edge of a track to set its height. Double-click the edge to show all rows again.
- If a track has more than 200,000 shapes to draw in the view, it shows "There is too much to draw here. Zoom in." in place of the data.

### Tracks panel

Push Ctrl+Shift+T (Cmd+Shift+T on macOS) or click the list button in the title bar to show the tracks panel at the left of the tracks. The palette has **Show the tracks** too.

- The panel has a row for each track, in the order of the tracks, with its color and its name. A track that overlays another track shows the name of the other track.
- Click the eye of a row to hide the track or to show it again. A hidden track does not show in the view or in exports, and genie does not read its data. Sessions keep hidden tracks. **Hide** in the track menu also hides a track.
- Type words in **Find a track** to show only the rows whose names have all the words.
- Right-click a row for the track menu.
- Drag the name of a row to change the order of the tracks. While you drag, the panel shows the new order. A track in a folder moves among the tracks of its folder, and a loose track or a folder moves among the loose tracks and the folders.
- To put a track in a folder, right-click the track in the panel or in its lane, then click a folder under **Folder**, or click **New folder…** and type the name. The track goes after the other tracks of the folder. **Out of the folder** takes it out.
- A folder row shows the number of its tracks. Click its arrow to fold or unfold its rows, and its eye to hide or show all its tracks in one step. Right-click it to rename the folder or to remove it. The tracks of a removed folder stay. Sessions keep the folders.

## Variant table

- Right-click a variant track and click **Variant table** to see its variants in a table under the tracks. The palette has **Show the variant table**. Click the item again, or click ×, to close the table.
- The table has a row for each variant in view of the first locus. The columns are the chromosome, the position, the ID, the reference allele, the alternate alleles, the number of heterozygous and homozygous alternate calls (if the file has samples), Quality, Filter, the annotation fields and the INFO fields. If the track has a filter, the table shows only the variants that the filter keeps.
- Click a column name to sort the rows by that column: numbers in order, then text. Click again to reverse the order, and a third time to show the order of the file again. Empty cells are always last.
- Type in **Find in the table** to show only the rows with a cell that contains the text. The search ignores letter case.
- Click a row to go to its variant.
- **Export TSV** saves the rows in their order, with all columns, as a tab-separated file.
- If the view is too wide to show the variants, the table asks you to zoom in.

## B-allele frequency

The B-allele frequency of a sample at a site is the fraction of its reads that carry an allele that is not the reference allele. A heterozygous site is near 0.5, and a homozygous site is near 0 or 1. A change of copy number or a loss of heterozygosity moves the heterozygous sites away from 0.5.

- Right-click a variant track and click **B-allele frequency**. A lane between the sites and the rows of the samples shows a dot for each site of the first sample of the file, from 0 at the bottom to 1 at the top, with a line at 0.5. The lane shows the sites where the sample has a call and 10 reads or more.
- To show a different sample, click its name under **B-allele frequency** in the track menu. The menu lists up to 8 samples around the sample that shows. The palette has **Show the B-allele frequency of** each sample, for example **Show the B-allele frequency of NA12878**. To remove the lane, click **B-allele frequency** again, or use **Remove the B-allele frequency lane** in the palette.
- genie gets the fraction from the AD field of the sample: the reads of the other alleles, divided by all its reads. If the sample has no AD values, genie uses its VAF, AF or BAF field, as DeepVariant, Mutect2 or a genotyping array writes it, and its DP field for the reads. A site without reads in the file shows too.
- In a wide view, the lane is under the density of the track, up to a whole chromosome, if the data of the view is 32 MB or less (8 MB for a file on the web). With more data, the lane tells you to zoom in. genie reads all of a file without an index one time for each sample.
- When you zoom out, the sites in one pixel at the same height show as one dot.
- Put the pointer on a dot to see the sample, the fraction, the reads and the call.
- The lane does not use the filter of the track. Sessions, images and undo keep the lane and its sample.

## Depth ratio

The depth ratio of two read files shows where the copy number changes, for example in a tumor sample against a normal sample of the same person.

- Right-click a read track and click the other read track under **Compare depth with**. A depth ratio track opens under the first read track, with the names of the two tracks, as "tumor.bam / normal.bam". The palette also has **Compare the depth with** each other read track. To remove the ratio track, click the name again, or remove the track.
- Each dot is the log2 ratio of the depths of the two files in a bin: blue under 0 and vermillion over 0, as in copy number tracks. A line marks 0, where the two depths are the same. genie divides the depth of each file by its usual depth, the median of the bins of the chromosome. Thus a file with more reads does not move all the dots up.
- In a view wider than 1 Mb, genie gets the depth of each bin of 16 kb from the index of the file alone, as goleft indexcov does: from the bytes between the windows of a BAI index, or from the bytes of each slice of a CRAI index. genie reads no reads, also for files on the web. A BAI index must have the counts of reads that samtools index adds.
- In a view of 1 Mb or less, genie counts the reads that start in each bin, in bins of 4 pixels and 500 bases or more, if the reads of the view are 32 MB or less (8 MB for a file on the web). Only mapped primary reads count. With more data, the bins come from the indexes.
- Lines in the color of the text show the segments: parts of the chromosome with one ratio. genie finds them in the bins of the whole chromosome from the indexes, by binary segmentation, a simple form of the circular binary segmentation (CBS) of DNAcopy. genie divides a part at the place where a t-test gives the smallest p, while p multiplied by the number of places is less than 0.001 and each new part has 5 bins or more. The line of a segment is at its mean log2 ratio.
- Put the pointer on a dot to see the bin, its log2 ratio, the two depths over their usual depths, and where the depths come from. Put the pointer on a line to see the mean log2 ratio of the segment, its bins and its length.
- The lane shows the log2 ratios from −2 to +2. To change the range, type a number in **Range ±** in the track menu and push Enter. **Default range (±2)** goes back. Drag the bottom edge of the track to make it taller.
- Sessions keep the ratio track, its two read tracks and its range. A read track with a lift cannot be in a ratio.

## Variant review

Look at the variants of a file one after the other, for example the calls of a variant caller with the reads under them, and mark each one true, false or not sure, with a note.

- Right-click a variant track and click **Review variants…**. The palette also has **Review the variants**. A bar under the tracks shows the first variant from the left edge of the view, in the middle of the view. When the view is wider than 1 Mb, genie shows 200 bp around each variant.
- Push T to mark the variant true, F to mark it false and U to mark it not sure, or click **True**, **False** or **Not sure** in the bar. **Clear** removes the mark.
- Type a note in the note field of the bar. The note goes with the mark. Push Enter in the field to use the keys of the review again.
- Push N or P, or click the arrows, to go to the next or the previous variant of the file, as **Next variant** and **Previous variant** do. They skip the variants that the filter of the track hides. For example, hide the Review values true, false and not sure to go only to the variants without a mark.
- A strip over the sites shows the mark of each variant: green for true, vermillion for false and grey for not sure. The card and the variant table show the fields **Review** and **Review note**, and filters can use them.
- **Export…** saves the marks as a TSV file with the chromosome, the position, the ID, the alleles, the mark and the note. For a VCF file on the computer, it can also save a copy of the file with the INFO fields REVIEW and REVIEW_NOTE on the marked variants. A name that ends in .gz gives a copy with bgzip. genie does not write over the open file.
- Sessions and undo keep the marks. genie cannot review the variants of a lifted track.

## LD

LD (linkage disequilibrium) tells how often the alleles of two variants go together in the samples of a file. genie shows the r² of each pair of variants in view, as Haploview does, and colors the dots of GWAS tracks by their r² with a lead variant, as LocusZoom does.

- Right-click a variant track with two samples or more, for example a VCF file of the 1000 Genomes Project, and click **LD**. A lane under the sites shows a cell for each pair of variants in view, under the middle between them, and lower for variants farther apart. The color goes from light grey for an r² of 0 to vermillion for 1. Put the pointer on a cell to see the two variants and their r².
- genie gets the r² from the genotypes, without phase: the squared correlation of the counts of the alternate allele (0, 1 or 2) over the samples. A sample without a call at one of the two variants does not count for the pair. A variant with the same count in all samples has no r².
- The lane shows when the view has 200 variants or less, and genie finds the r² of the loaded variants when they are 1,000 or less. Zoom in when the lane tells you to.
- LD differs between populations. To use a group of the samples, open a sample table and group the samples by a field, for example the super_pop field of the 1000 Genomes Project (see [Sample groups](#sample-groups)). Then right-click the track and click a group under **LD**, for example **EUR**. **All samples** goes back. The palette has **Show the LD of** each group.
- While a variant track shows its LD, the dots of the GWAS tracks take the LocusZoom colors of their r² with the lead variant: red from 0.8, orange from 0.6, green from 0.4, light blue from 0.2, dark blue under 0.2, and grey when the variant of the dot is not in the variant track. genie finds the variant of a dot by its position. The lead is the top dot in view, as a purple diamond. To pick another lead, click a dot to keep its card open, then click **Lead**.
- The LD lane does not use the filter of the track. Sessions and undo keep the LD, its group and the lead.

## Sashimi plots

A Sashimi plot shows the coverage of an RNA-seq read track and the splice junctions between its exons, with the number of reads of each junction.

- Right-click a read track and click **Sashimi plot**. The track then shows the coverage as an area in the color of the track, and each splice junction as an arc with its number of reads. The first junction is above the coverage, the next one is below it, and so on, so that near arcs do not hide each other. A thicker arc has more reads.
- **Arcs ≥** in the track menu hides the junctions with fewer reads, for example 5 or 10.
- To compare samples, show each read track as a Sashimi plot, give each track a different color, and turn on **Shared scale** in the menu of each track. The tracks then use the same range of coverage.
- Put the pointer on an arc to see its junction: the position, the reads, the strand and the length of the intron.
- To save the plots as an image, push Ctrl+E (Cmd+E on macOS), click Export… and save a PNG or an SVG file. Put a feature track with the transcripts under the read tracks to show them in the image.
- Click **Sashimi plot** again to see the reads. Sessions keep the Sashimi plot, the **Arcs ≥** setting and the shared scale of each track.

## Isoforms

The isoforms of a read track with long RNA reads, for example PacBio Iso-Seq or Oxford Nanopore cDNA reads, are the groups of reads with the same splice junctions.

- Right-click a read track and click **Isoforms**. The palette also has **Isoforms**. The track then shows the coverage, and under it a row for each isoform in view with its exons, its name and its number of reads in view. The isoform with the most reads is at the top.
- Two reads are in the same isoform when they have the same number of splice junctions, and each junction starts and stops in 5 bases or less of the same junction of the other read. The exons of an isoform go from the first base of its reads to the last.
- Many long reads do not start at the first exon. A read can be a part of a longer isoform: its junctions are the same as a run of junctions of that isoform. Then the read counts for that isoform if its ends stay in the exons at the two ends of the run. If it fits more than one isoform, the isoform with the most reads gets it.
- When a feature track with transcripts is in view, genie first moves the ends of the junctions of the reads. An end that is 20 bases or less from the end of an intron of a transcript moves to that end, because aligners can put the splice sites of noisy reads some bases off.
- If a transcript has the same introns as an isoform, in 5 bases or less, the isoform has the name of the transcript. When the track shows gene names in place of transcript IDs, as GENCODE V48 does, the name also has the ID, for example "HNRNPA1 ENST00000546500.5 · 1,334 reads". A transcript with an ID gives the name before a transcript without one. If no transcript has the same introns, the name is the gene of the transcript that has the most of the junctions of the isoform, with "novel 1", "novel 2" and so on for that gene, for example "HNRNPA1 novel 1". When no transcript has one of its junctions, the name is "novel 1", "novel 2" and so on. An isoform whose reads give no strand (XS or ts tags) gets the strand of the transcript that gives its name.
- An isoform gets a row when it has 2 reads or more in view and 1 % or more of the spliced reads in view. The line under the rows counts the other isoforms and their reads, and the reads without a splice junction.
- Put the pointer on a row to see the card of the isoform: its name, its place, its length and its number of exons.
- Click **Isoforms** again to see the reads. The Sashimi plot and the isoforms do not show at the same time. Sessions keep the isoforms of each track.

## Isoform table

- Right-click a read track and click **Isoform table**, or use **Show the isoform table** in the palette. A table under the tracks shows the isoforms of the rows of the **Isoforms** lane in view, also when the track shows its reads: the name, the kind, the place, the reads in view, their share of the spliced reads in view, the exons and the length of the exons. **Show all** adds the isoforms with fewer reads, in grey.
- Click a row to go to the isoform. Click the name of a column to sort the rows by it, click it again to sort them the other way, and a third time to show the isoform with the most reads first again.
- The kind compares the junctions of the isoform with the introns of the transcripts of the open feature tracks, as the structural classes of SQANTI: **known** (the same introns as a transcript), **part of a known** (the same introns as a run of introns next to each other of a transcript), **new combination** (each start and each end of the junctions is a start or an end of a known intron) and **new junction** (a start or an end that no transcript has). Without a transcript in view, the kind is empty.
- When the reads are in groups, for example with **Group by phase**, a column for each group gives the reads of the isoform in the group and their share of the spliced reads of the group in view. The 6 largest groups get a column, and the other groups share the column "Other". When the reads have exactly two groups besides the reads without a haplotype or a value of the tag, as haplotype 1 and haplotype 2, the p column gives the two-sided Fisher's exact test of the reads of the isoform against the other spliced reads of the two groups. A small p shows an isoform that one group uses more than the other, for example an isoform of one allele.
- **Export…** saves the rows in the order of the table. The end of the file name gives the format. .bed gives BED12: the ID as the name (the transcript ID, or for example HNRNPA1_novel_1), the reads up to 1,000 as the score, and the exons as the blocks. .gtf gives GTF: a transcript line and exon lines for each isoform, with gene_id, transcript_id, reads and kind. .tsv gives all the columns with a tab between them, the place in four columns and the shares from 0 to 1. A file with another end gets .bed.
- One table shows at a time: the variant table, the DMR table or the isoform table. The table closes when its track closes.

## Phase reads

The reads of a sample can have an HP tag with their haplotype, for example from whatshap haplotag. genie can give a haplotype to the reads without an HP tag from the phased SNVs of an open VCF file.

- Open the read file and a VCF or BCF file with phased genotypes, for example `0|1` from whatshap phase, HiPhase or LongPhase. Right-click the read track. **Phase by** lists the variant tracks with samples. Click one. genie uses the sample with the name of a sample of the reads (SM in the @RG lines of the header), else the first sample. Up to 8 samples show under the variant track: click a sample to use it.
- At each phased heterozygous SNV of the sample, a base of the read with a quality of 20 or more votes for the haplotype with that base. The votes of each phase set (PS) count apart, and the phase set with the most votes decides. A read gets haplotype 1 or 2 when it has 2 votes or more for it, and twice the votes for the other haplotype. Other reads have no haplotype. Each read counts alone: the two mates of a pair do not add their votes.
- Reads with an HP tag keep their haplotype. The card of a read with a haplotype from the VCF shows "HP 1 from the VCF" or "HP 2 from the VCF".
- When some reads have an HP tag, the haplotypes from the VCF take the names of the tags. In each phase set, the reads with an HP tag and a haplotype from the VCF vote. When most of them disagree, for example because the tags come from another phasing, genie swaps haplotype 1 and 2 of the VCF in that set. So the reads of one haplotype are in one group.
- Then **Group by phase** shows the reads of haplotype 1, of haplotype 2 and the reads without a haplotype, and **Methylation difference** and the **DMR table** compare the two haplotypes.
- genie reads the phased SNVs of the reads in view and of 100 kb on each side. When the records of that region are more than 32 MB (8 MB for a file on the web), the reads get no haplotype from the VCF. genie reads all of a file without an index one time for each sample.
- Click the variant track again to stop. The palette has **Phase the reads by** each variant track and **Stop phasing the reads**. Sessions, images and undo keep the variant track and the sample. When the variant track closes, the reads lose the haplotypes from it.

## Single-cell

The reads of single-cell RNA-seq and ATAC-seq have the barcode of their cell: the CB tag of 10x Genomics, or the XC tag of Drop-seq. A table of the cells gives each barcode a cell type.

- Write the table of the cells in R or Python: `write.csv(seurat@meta.data, "cells.csv")` for Seurat, or `adata.obs.to_csv("cells.csv")` for Scanpy. genie takes the barcodes from the first column whose values are mostly barcodes. The file can be CSV or TSV, also compressed with gzip, and up to 256 MB.
- Right-click a read track or a fragment track, click **Cell types…** and pick the table. genie uses the column whose name says cell type best, for example cell_type, predicted.celltype.l2, seurat_annotations or seurat_clusters. **Cell types by** in the track menu lists the columns with 2 to 100 values: click one to use it.
- The barcode of a read finds its cell in the table by the same text, or else by its core: its longest run of A, C, G and T. Then a barcode with a sample prefix, as pbmc1_AAACCCAAGAAACACT-1 from a Seurat merge, or with another suffix, as AAACCCAAGAAACACT-1-0 from Scanpy, finds the reads with AAACCCAAGAAACACT-1. When cells of other types have the same core, the reads have no cell type.
- **Group by cell type** puts the reads in rows by the cell type of their barcode. Reads without a barcode, or with the barcode of no cell of the table, are in "No cell type".
- **Coverage by cell type** shows the coverage of the reads, and under it a lane for each cell type: the coverage of the reads of its cells divided by its number of cells in the table, on one scale for all lanes, as the CoveragePlot of Signac. The 12 cell types with the most cells have a lane each, and the other types share the lane "Other". genie counts all the reads, also the reads that down-sampling hides. Put the pointer on a lane to see its coverage and its coverage per cell.
- When the table has the cells of more than one sample, the lanes divide by the cells of all samples. For the coverage per cell of one sample, use the table of that sample.
- genie opens the fragment files of single-cell ATAC-seq, as the `fragments.tsv.gz` of Cell Ranger ATAC, with their tabix index. It also reads a small file without an index, up to 256 MB. A fragment track shows one lane with the coverage of all fragments, or with a cell table a lane for each cell type, as above. It shows views of 2 Mb or less, or 500 kb for a file on the web.
- The palette has **Cell types…**, **Cell types by** each column, **Group by cell type**, **Coverage by cell type** and **Remove the cell table**. Sessions and images keep the cell table, its column, the groups and the lanes of each track.

## Reference tracks

genie can make tracks from the bases of the open reference, without a file: the GC content, the CpG islands, the repeats and the sites of restriction enzymes. They help most with a genome that has no tracks of its own, for example a new assembly in a FASTA file.

- Right-click the sequence and click a track under **Add a track**. The genie menu has the same tracks under **Reference tracks**, and the palette has **Add a GC % track**, **Add a CpG islands track**, **Add a repeats track** and **Add restriction sites…**.
- The tracks show when the view is 10 Mb or less. They work as other tracks: you can move, rename, hide and filter them, and sessions, images and undo keep them.
- **GC %** is a signal track. Each bar shows the share of G and C bases in windows of 100 bases. Click 5, 20, 500 or 1,000 under **Window (bp)** in the track menu to use other windows. When you zoom out, a bar shows the mean of its windows, and a light color goes up to the highest window. A window with more N bases than known bases has no value.
- **CpG islands** finds islands as the CpG Islands track of UCSC finds them. Each CG scores +17 and each other pair of bases scores -1, and the segments with the highest scores are the candidates. An island is a candidate of more than 200 bases, with 50 % GC or more and a ratio of observed to expected CpG of more than 0.6. genie uses all bases, also the soft-masked bases, as the unmasked CpG islands of UCSC do. The name of an island is "CpG:" and its number of CpGs. Its card shows the CpGs, the C and G bases, the percent of CpG and of GC, and the observed/expected ratio.
- **Repeats** shows the soft-masked bases of the reference: the runs of lower-case bases. Most genomes from UCSC and the `dna_sm` files of Ensembl mark their repeats in this way, for example the repeats that RepeatMasker and Tandem Repeats Finder find. A reference with only upper-case bases has no repeats. The track is squished at first. A repeat that goes on past the 20 kb that genie reads on each side of the view has a note in its card.
- **Restriction sites…** opens a list of 60 common enzymes. Click enzymes to pick them, or type a site with ^ at the cut, for example `G^AATTC`, and click **Add**. A site can have IUPAC codes, such as N. Then click **Add the track**. When the track has more than one enzyme, each enzyme has its own color. Click **Enzymes…** in the track menu to change the enzymes.
- The card of a site shows the site, the base after which the enzyme cuts, and the lengths of the fragments to the nearest cuts before and after it. A fragment that goes past the 20 kb that genie reads on each side of the view shows its least length, for example "more than 21000". Filters work on these fields, for example to keep the cuts with a fragment of 500 bases or more after them.

## Motif sites

A motif file has the weight matrices of DNA motifs, for example the binding sites of transcription factors from JASPAR, MEME or HOMER. genie finds the sites of the motifs in the reference, on the two strands, as FIMO of the MEME suite does.

- Open the motif file as a track. A dialog lists its motifs, each with its name, its ID, its length and its consensus. Type a name or an ID, for example CTCF or MA0139, to find a motif. Click motifs to pick them, then click **Add the track**.
- The track shows the sites of the picked motifs when the view is 1 Mb or less. If you pick no motif, the track shows the sites of all motifs of the file when the view is 5 kb or less, because many motifs take more time. When the track has more than one motif, each motif has its own color.
- A site is a window of the reference whose score has a p-value under 0.0001. genie finds the exact p-value of each score from the weight matrix, for the chances of the bases in the background: the background of a MEME file, or 25 % for each base. As FIMO does, genie adds a pseudocount of 0.1 to the matrix. A window with a base that is not A, C, G or T is not a site.
- Click **1e-3**, **1e-4** or **1e-5** next to **p <** in the track menu to show more or fewer sites. Click **Motifs…** to change the motifs.
- The card of a site shows the motif ID, the score in bits, the p-value and the bases of the site on the strand of the motif. Filters work on these fields, for example to keep the sites with p at most 0.00001.
- Sessions keep the picked motifs and the threshold of each motif track.

## Dot plot

A dot plot compares two assemblies with the alignments of a [PAF file](#paf-alignments), for example from `minimap2 -cx asm5 genome.fa other.fa`. The target sequences, which are the sequences of the reference in view, go across. The query sequences go down.

- Right-click a PAF track and click **Dot plot**. The palette also has **Show the dot plot**. The plot shows in place of the tracks. genie reads all lines of the file, so a large file can take some time.
- Each alignment is a line from its first base on the target to its last. The line goes down for the forward strand and up for the reverse strand, so an inversion is a line that goes up. Blue is the forward strand and vermillion is the reverse strand.
- Click **Color by identity** to color the lines by their identity: dark for the highest identity, and light for the identity that one alignment in 20 is under, or less. The bar over the plot shows the two identities. Click **Color by strand** to go back.
- The targets are in the order of the reference. Each query goes next to the place on the targets where most of its bases align. genie does not turn the queries around.
- The names of the sequences show where they fit. A thin line shows where one sequence stops and the next one starts.
- Put the pointer on a line to see the card of its alignment: the query and the place on the query, the place on the target, the identity, the matches, the length of the block, MAPQ and the type.
- Drag a box to zoom in to it. Click **Whole plot** to see all of the plot again.
- Click a line to show its alignment in the tracks. Back returns to the plot. Back and forward also step through the zooms of the plot.
- The plot shows only the alignments that the filter of the track keeps, for example the alignments with **Identity (%)** at least 95, or with **Type** primary. When the filter changes, genie reads the file again.
- Click × at the right of the bar to show the tracks again.

### Synteny view

Click **Synteny view** in the bar of the dot plot to see the same alignments in two rows: the targets in a row at the top and the queries in a row at the bottom, in the order of the plot. A band joins the two parts of each alignment. The band of a reverse alignment crosses over itself, so an inversion shows as a cross.

- Drag along the top row to zoom in to a part of the targets. The bottom row then shows the parts of the queries that align with it. Drag along the bottom row to zoom in to a part of the queries. A zoom leaves out the parts at the two ends of the other row that hold one aligned base in 20, so that a few repeats far away do not stretch the row.
- The view shows only the parts of the alignments that have both ends in view.
- Put the pointer on a band to see its card, and click it to show the alignment in the tracks, as in the dot plot. The colors, the filter and **Whole plot** also work as in the dot plot.
- The two views share the zoom. Click **Dot plot view** to go back.

## Liftover

A chain file moves data from the places of one assembly to the places of a different assembly, for example from hg19 to hg38. UCSC has chain files in the `liftOver` folder of each genome, for example `hg19ToHg38.over.chain.gz`.

- Right-click a track and click **Lift over…**. Then open a chain file from the assembly of the track to the assembly in view, plain or compressed with gzip. The chain file and the data file can name the chromosomes differently, for example `chr1` and `1`.
- genie reads only the parts of the data file that align with the view. Each feature, variant, value, link, segment and dot goes to its place in view: the span from its first to its last aligned base, on the chain that aligns the most of its bases. If that chain aligns less than half of its bases, genie does not show it. Exons and coding parts move with their feature. If the chain aligns the two assemblies on opposite strands, a feature changes its strand, and a variant shows the reverse complement of its alleles.
- The locus box finds the feature names and the variant IDs of a lifted track at their new places.
- genie cannot lift read tracks and Hi-C tracks. A lifted track does not have **Next feature** and **Previous feature**.
- Click **Remove lift** in the track menu to show the data at its own places again.
- Sessions keep the chain file of each track. genie reads the chain file again before it shows the track.
- Click **Lift…** under the region list to move the regions from the places of a different assembly to the places of the assembly in view, for example after you import a BED file of hg19 regions into an hg38 session. A region that the chains do not move leaves the list, and a notice gives the number. If the chains move none of the regions, the list does not change.

## Sample groups

A sample table gives facts about the samples of variant and copy number tracks, for example the population, the sex or the phenotype. genie uses the table to put the rows of the samples in groups, to sort them and to color them.

- Open a sample table as you open a track: open the file or drop it on the window. genie reads tab or comma separated files with a header line (`.tsv`, `.csv`, `.txt`, and the `.panel` files of the 1000 Genomes Project), IGV sample information files (the `#sampleTable` part) and PLINK `.fam` and `.ped` files. The column with the name `sample`, `ID` or `IID` has the sample names. If there is no such column, the first column has them. The other columns are the fields.
- In a clinical file from cBioPortal (for example `data_clinical_sample.txt`), the `SAMPLE_ID` column has the sample names, and the fields have the names of the first line. Values such as `[Not Available]` count as missing.
- genie reads the first lines of a `.txt` file to know what it is, also for a file on the web. A file that starts with a `hub` line opens as a track hub, and a loop list (`chr1 x1 x2 chr2 y1 y2 …`) opens as an interaction track. Other `.txt` files open as sample tables.
- For a PLINK file, the fields are the family, the father, the mother, the sex (male or female) and the phenotype (affected or unaffected, or the number of a quantitative phenotype).
- One table applies to all tracks of the session that have its samples. A notice tells how many samples of the open tracks are in the table. Open a different table to replace it. **Close the sample table** in the palette removes it.
- Put the pointer on a genotype row or a copy number row to see the fields of its sample in the card. Click the row to keep the card open. Then click a field value, and click **Group by this field**, **Sort by this field** or **Color by this field**.
- A group shows the rows of one value together, under a band with the value and the number of samples, for example "EUR · 503 samples". Numbers come in order, then text values. A track shows at most 30 groups: the samples of the other values go together as "Other ... values". After them come the samples without a value ("No ...") and the samples that are not in the table ("Not in the sample table").
- Sort puts the rows of each group in order: numbers from the smallest, text from A to Z, and samples without a value last.
- Color gives each row a strip at the left. The 8 most common values get 8 colors, and the other values are grey. For a field of numbers, the strip goes from light grey for the smallest number to blue for the largest.
- While a field groups a variant track, the card of a site shows the frequency of the other alleles (AF) in each group and the number of called alleles, for example "EUR · AF 0.12 · 1,006 alleles". genie counts two alleles for each called sample. The card shows up to 10 groups.
- The track menu shows the fields in use, for example **Group samples by pop**. Click one to stop. The palette has **Group samples by**, **Sort samples by** and **Color samples by** for each field and each track.
- If each row of a big cohort shows a group of samples, all samples of a row are in the same group.

### Trios

If the sample table gives the father and the mother of a sample, and the variant track has the three samples, genie checks each variant of the child. The columns of the father and the mother can be the fields of a PLINK file, or columns with the name `Father` and `Mother`, `Paternal ID` and `Maternal ID`, or `paternal_id` and `maternal_id`.

- The card of a variant has the field **Inheritance**. **Mendelian error** tells that the child cannot have its call from its parents. **De novo** tells that the child is heterozygous and both parents are homozygous reference: the child can have a new mutation. A de novo variant is also a Mendelian error. **Consistent** tells that the calls agree. A variant with a missing call in a trio has no field for that trio.
- A frame shows around the call of a child with a Mendelian error.
- The card of the child's call tells the calls of the parents, for example "De novo: father homozygous reference, mother homozygous reference".
- The track menu has **Only de novo variants** and **Only Mendelian errors**. They add a filter on Inheritance. Then Ctrl+F and Ctrl+B go to the next and the previous variant that the filter keeps, and the variant table shows only these variants. The menu shows the filter, and you can remove it there.
- At a site with more than one alternate allele, a heterozygous call can have two alternate alleles. There, genie tells only the errors that no alleles can explain.
- A son has one X chromosome, from his mother. So genie checks X only for daughters, and it does not check Y and the mitochondria. The sex of a child comes from the PLINK sex or from a field with the name `Sex` or `Gender` (male, female, M, F, 1 or 2).

## Undo

Push Ctrl+Z (Cmd+Z on macOS) to undo the last work on tracks and regions: open, remove, move or change a track, filter it, overlay or compare it, group its samples, lift it, or add, rename or remove a region. Push Ctrl+Y or Ctrl+Shift+Z (Cmd+Shift+Z on macOS) to do it again. The palette has **Undo** and **Redo** too.

- genie keeps the last 100 steps of the open reference. A new step drops the steps that you can redo.
- Moves of the view are not steps: Back and Forward go to earlier views.
- A track that comes back loads its data again.

## Sessions

To save the reference, the tracks (with their names, colors, ranges, overlays, sample fields and chain files), the view, the regions, the track hubs and the sample table as a session, push Ctrl+S (Cmd+S on macOS) or click the save button. To open a session, open its `.genie` file. genie keeps the last session when you close it, and the start view can restore it.

If a file is in the folder of the session file, the session file has its path relative to that folder.

## Export

To save the view as an image, push Ctrl+E (Cmd+E on macOS) or click the camera button. The export dialog opens:

- **Title**: the title at the start of the first line of the image. Keep it empty for no title.
- **Scale bar**: a line of a round length, for example 10 kb, at the end of the first line. If the view has several loci, each panel has its own scale bar.
- **Region bands**: the regions in view (see [Regions](#regions)), as grey bands under the ruler. The name of a region is in its band, or next to it if the band is too short. Regions that overlap go on different rows.

Push Enter or click Export…, then select the file. The session keeps the title and the two settings.

The image shows the locus, the ruler and all tracks, also the tracks that are not on the screen. If the view has several loci, the image shows each panel with its locus and its ruler. In the whole-genome view, the image shows the name of each chromosome. The image uses the current theme.

- If the file name ends with `.svg`, genie saves an SVG image. The SVG has shapes and text that you can edit, for example in Inkscape or Illustrator. The text uses the fonts Inter and JetBrains Mono. If a computer does not have these fonts, it uses similar fonts.
- Other names give a PNG image. The scale of a PNG image is 2. Its maximum height is 16,384 px. If the view is taller, the image shows only the top part, and genie tells you.

## Images from the command line

To save one image for each region in a BED file, without a window:

```bash
genie snapshot work.genie regions.bed pictures
```

The command to use depends on how you got genie:

- Windows: genie.exe has no console. In a terminal, use `genie-cli.exe` from the same folder, for example `& "$env:LOCALAPPDATA\genie\genie-cli.exe" snapshot work.genie regions.bed pictures` in PowerShell.
- macOS: `/Applications/genie.app/Contents/MacOS/genie snapshot work.genie regions.bed pictures`.
- Linux: `genie snapshot work.genie regions.bed pictures` after you install the .deb, or `./genie-0.3.1-linux-x86_64.AppImage snapshot work.genie regions.bed pictures`.
- Source code: `cargo run -p genie -- snapshot work.genie regions.bed pictures`.

- genie opens the session (the genome or reference, the tracks and their settings). Then it goes to each region and saves a PNG file in the folder. If the folder does not exist, genie makes it. Each image shows one region, also if the session has several loci.
- The name of an image is the name of the region (BED column 4) or the locus, for example `chr1_1001-2000.png`. If two regions have the same name, genie adds `-2`, `-3` and so on.
- The title of an image is the name of its region. For a region without a name, the title is that of the session. The scale bar and the region bands follow the session too. To set them, use the export dialog and save the session.
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

## Updates

When genie starts, it asks GitHub for the newest release, at most once a day. If a newer version exists, a notice says so. Click **Download** to open the release page in your browser, then install the new version as usual.

- The check sends one request to api.github.com. The request contains the version of genie. genie sends no other data.
- To stop the checks at start, open the settings and set **Updates** to **Off**. **Check now** in the settings, **Check for updates…** in the genie menu and the palette always check.
- The settings show the version and the result of the last check, for example "genie 0.3.1 · Up to date".

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

The tests give cut and damaged copies of each test file to the readers: 3 copies of each file. Each reader must give an error or data, and must not panic or hang. The full sweep makes about 24 copies of each file and takes a few minutes:

```bash
cargo test -p genie --bin genie sweep -- --ignored
```

## Website

The project page in `docs/` comes from the pages in `site/`, this README (the manual), the changelog, the keys of genie and its logo. A test fails when a file in `docs/` is out of date. To write the files again:

```bash
GENIE_WRITE_SITE=1 cargo test -p genie --bin genie site
```

The clips of the tutorial and the pictures are files in `docs/clips/` and `docs/images/`. To record the clips again after a change of the look, run this PowerShell script (`pwsh` on macOS and Linux). It needs ffmpeg with libx264 and libwebp:

```bash
powershell -File site/clips/record.ps1
```

genie runs without a window, and a script in `crates/genie/src/app/clips.rs` moves the pointer and clicks. The clips use real data from the web: a first run of all clips fetches about 35 MB. The work folder `target/clips` keeps the fetched ranges, so a later run fetches only new ranges. `-Clips cards, hero` records some of the clips, `-Cap 60MB` changes the most bytes that all runs can fetch (50 MB), and `-SkipRecord` encodes the frames of the last run again.

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
