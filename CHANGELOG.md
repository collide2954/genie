# Changelog

## Unreleased

### Methylation

- When **Color by modification** is on, a lane above the reads shows the share of modified reads at each site. Grouped reads get a lane for each group, for example one for each haplotype.
- At a CpG, genie adds the calls of the two strands together.
- The read track menu selects the modification of the lane, for example 5mC or 5hmC.
- genie opens bedMethyl files from modkit as signal tracks that show the percent of modified reads. The track menu selects the modification and joins or separates the two strands of each CpG. The card of a site shows its counts.

### Easier to use

- Click the genie logo for a menu: Open file, Open URL, Recent, Save session, Export image, Keys and mouse, and Settings. Ctrl+O (Cmd+O on macOS) opens a file.
- Ctrl+K (Cmd+K on macOS) opens a palette. Type words in any order to find an action, a track item or a recent file, then push Enter.
- The ? button and F1 show all keys and mouse actions.
- Buttons next to the locus box zoom out, zoom in and show the whole chromosome.
- Put the pointer on a track to see a button for its menu and a button that removes it.
- **Track settings…** in the track menu shows all settings of the track in one dialog. Each change shows at once.
- Menu items show their keys, for example "Next feature Ctrl+F".
- A strip under the ruler shows tips until you close it.
- The logo has one strand in green and purple bases.

### Find a sequence

- Ctrl+Shift+F (Cmd+Shift+F on macOS) opens a find bar. Type bases, for example a primer, with IUPAC codes if necessary. A lane under the ruler shows the matches on the two strands.
- Allow 1, 2 or 3 mismatches to also find near matches. The card of a match shows its strand and its wrong bases.
- Enter and Shift+Enter go to the next and the previous match. Past the view, genie reads the reference 20 Mb at a time.

### Filters

- Next feature and Previous feature (Ctrl+F and Ctrl+B) skip the features and variants that the filter hides.
- For a number in a card, type your own limit in the menu, then click At least or At most.
- If you type a name that a filter hides, genie goes to it and a note says so.
- A filtered bigBed track wider than 10 Mb shows the density of the features that the filter keeps, if their data in view is 16 MB or less.
- A click inside a menu does not close the menu.

### CRAM

- genie opens CRAM 3.0 and 3.1 files as read tracks, with gzip, rANS and rANS Nx16 compression. It decodes the reads with the reference that is open, and checks the MD5 of each part of the file against that reference.
- A CRAM file without its `.crai` index gets one in the cache folder.

### Track hubs

- genie opens UCSC track hubs from the web or from your computer. Use the tracks button, the genie menu, the palette or Open URL. A dialog shows the tracks of the open genome in their groups. Type words to find a track. Click a track to add or remove it.
- genie adds bigBed, bigWig and the other UCSC big formats, bam, cram and vcfTabix tracks, with the type and the index that the hub gives.
- If the hub has no tracks for the open genome, the dialog lists its genomes. A click opens the genome, also the 2bit sequence of an assembly hub, and then shows the tracks of the hub.
- Sessions keep their hubs. The tracks button lists the hubs that you opened last.
- The tracks button is on for all genomes and reference files.

### Interaction tracks

- genie opens bigInteract and BEDPE files as interaction tracks. Each link is an arc between its two ends: longer links rise higher, and links with a larger value have a thicker line.
- Arcs with an end outside the view go past the edge. A link to another chromosome shows as a stub.
- The card of a link shows its two ends, the distance, the value and the other fields. BEDPE header lines name the fields.
- The arcs use the colors of the file, or the track color.
- genie opens the loop lists of Juicer and HiCCUPS (`.txt`, also compressed and on the web). The header line names all fields, and the `color` column gives the color of each arc.
- genie opens bigInteract files with the older names of the ends (`region1` and `region2`).

### Copy number

- genie opens copy number segment files as heatmap tracks: SEG files (as from TCGA, DNAcopy or GISTIC), CNVkit `.cns` and `.cnr` files, and GATK ModelSegments `.seg` files.
- Each sample has a row: blue for a loss, vermillion for a gain, light grey near 0. The full colors start at a log2 value of ±1.5. The track menu sets a different range.
- Up to 20 samples show their names. More samples get thinner rows, down to 1 px.
- The card of a segment shows its sample, its length, its markers and its value.

### Updates

- At start, genie asks GitHub for the newest release, at most once a day. A notice with a **Download** button opens the release page when a newer version exists.
- **Check for updates…** in the genie menu and the palette, and **Check now** in the settings, check at once. The settings show the version and the result, and turn the checks at start off.

### Read tags and filters

- Read cards list MAPQ, the insert size of a pair and the small tags of the read as fields. Click a field to keep or hide reads by its value, or to keep reads with at least or at most a number.
- **Group by this tag** groups the reads by the value of any tag, for example CB or PS, with up to 30 groups. **Color by this tag** gives the 5 most common values the track colors.
- The read track menu hides reads under MAPQ 1, 10, 20 or 30, and shows secondary, supplementary and QC-failed reads on request. Sessions keep these settings.

### Whole genome

- Type `All` in the locus box, or click the new button right of the zoom buttons, to see all main chromosomes side by side, each as wide as its share of the genome.
- Signal, copy number, feature and variant tracks show their data or density on one scale for all chromosomes.
- Click a chromosome to show it, or drag across a part to show that region. Back, forward and sessions keep the whole-genome view.

### Densities

- Variant tracks wider than 1 Mb and feature tracks wider than 10 Mb show the number of variants or features per Mb. Before, they asked you to zoom in.
- For a file with a tabix or CSI index from htslib, genie estimates the density from the index alone, also for large files on the web.

### Protein changes

- A variant card shows the change of the variant on the transcripts of the open feature tracks in HGVS notation, for example "BRCA1 p.Arg1699Trp · c.5095C>T".
- genie names substitutions, new stop codons, frameshifts, and deletions and insertions that keep the frame. A variant near an exon edge shows "splice site". A variant out of the coding sequence shows its place and its c. position, for example "5′ UTR · c.-14G>A" or "intron · c.92+5G>A".
- A deletion of whole exons shows "exon deletion", and a deletion of the whole transcript shows "transcript deletion".
- As HGVS asks, genie moves an insertion or a deletion in a repeat to the 3′ end of the transcript, and names an insertion of the bases before it as a duplication, for example "c.417dup" or "p.Lys2dup".
- A change of the stop codon shows the extension, for example "p.Ter148GluextTer?". An allele with unknown bases (N) shows "p.?".
- The card shows up to 3 transcripts, with the protein changes first, and the number of the other transcripts.

### Sample groups

- genie opens sample tables: tab or comma separated files with a header line, the `.panel` files of the 1000 Genomes Project, IGV sample information files, and PLINK `.fam` and `.ped` files. One table applies to all variant and copy number tracks of the session.
- Genotype and copy number cards show the fields of their sample. Click a field to group, sort or color the rows by it. A group has a band with its value and its number of samples. A strip at the left of each row shows the 8 most common values in 8 colors, or a ramp from grey to blue for numbers.
- While a field groups a variant track, the card of a site shows the allele frequency of each group, for example "EUR · AF 0.12 · 1,006 alleles".
- The track menu and the palette turn the fields on and off. Sessions keep the table and the fields of each track.
- genie opens clinical files from cBioPortal as sample tables, with the names of their first line. Values such as `[Not Available]` count as missing.
- genie reads the first lines of a `.txt` file to know if it is a track hub, a loop list or a sample table, also for a file on the web.

### Structural variants

- Structural variants show over their full length in the colors of their type: dark grey for deletions, vermillion for duplications, green for inversions and purple for insertions. genie finds them by SVTYPE, by symbolic alleles such as `<DEL>`, and by breakends.
- Lines with ends show the confidence intervals of the start and the end (CIPOS and CIEND).
- Breakends (BND) are arcs to their other ends in a lane above the sites, or stubs when the other end is on another chromosome.
- The card says the type, the length, the other end of a breakend and if the call is imprecise, for example "Deletion · 2,345 bp · imprecise".

### Variant annotations

- Variant cards show the annotations of Ensembl VEP (`CSQ`), SnpEff (`ANN`) and bcftools csq (`BCSQ`): the 3 most severe, one line for each with the gene, the consequence, the impact and the HGVS changes, and the number of the others.
- The fields Consequence, Impact, Gene and Biotype list the values of all annotations of a variant. Click a value to keep or hide the variants with it, for example to keep only HIGH.
- The other columns of the first annotation show as fields, for example `CSQ SIFT` and `CSQ PolyPhen`.
- The annotations do not count in the 400 characters of INFO that genie keeps, so long annotations do not push out the other INFO fields.

### Cloud addresses

- genie opens files in public buckets of Amazon S3 and Google Cloud Storage from their `s3://` and `gs://` addresses, for example the 1000 Genomes data on AWS and the gnomAD data on Google Cloud. The addresses work in Open URL, in track hubs, in sessions and on the command line.
- If a bucket is not public, the message tells you that genie reads only public buckets.

### Packages

- The Windows installer and the macOS disk image do not ask you to accept the license. The packages contain the license as the file LICENSE.txt.

### Fixes

- A card breaks a word that is too long for one line, for example a long variant name. Before, the word went past the edge of the card.
- The Open dialog lists copy number files (`.seg`, `.cns`, `.cnr`) and BEDPE files with the tracks.
- For a genome on the web, genie reads only the repeat masks of the 2bit file near the view. Before, the first bases of each human chromosome read 2 to 3.5 MB of masks.
- A track that loads while you zoom in loads again for the new view. Before, a feature track could keep the density of the wide view.
- If two INFO descriptions give the same short label, the variant card shows the INFO IDs, for example `AF_EUR` and `AF_EUR_unrel`.
- Old bigBed files without a count of BED fields open. Before, they failed with "A number is not valid".
- The names of files on the web show their escaped characters, for example `+` for `%2B`.
- The message for a file that genie cannot open lists all track types.

## 0.2.0

genie opens track files as they come, also when they have no index or are not sorted.

### Files without an index

- genie opens track files that have no index. It makes the index in its cache folder for BAM files, and for VCF, BED, GFF3, GTF and bedGraph files compressed with bgzip.
- Plain and gzip track files of any size open. genie makes a copy compressed with bgzip, and it sorts files that are not sorted by position, for example GTF files from GENCODE or Ensembl.
- Track files on the web that have no index download into the cache folder, up to 1 GB. A stopped download continues from where it stopped.
- While genie prepares a file, the track shows the progress. Click × on the track to stop.
- The settings panel shows the space that the copies and indexes use. **Clear** removes them.

## 0.1.1

Fixes for Windows and the Linux AppImage.

- Windows: genie starts on computers that do not have the Visual C++ Redistributable. genie.exe now contains the C runtime.
- Windows: genie opens without a console window.
- Windows: the Start menu entry, the desktop shortcut and `.genie` files show the genie icon.
- Windows: `genie-cli.exe`, in the same folder as genie.exe, runs commands in a terminal, for example `genie-cli snapshot work.genie regions.bed pictures`.
- Linux: the AppImage finds files from the folder that you run it in, for example in `./genie-0.1.1-linux-x86_64.AppImage snapshot work.genie regions.bed pictures`.
- Before each release, the release workflow installs or opens each package and makes images with it.

## 0.1.0

The first release of genie, a genome browser for Windows, macOS and Linux.

### Genomes and files

- Ten genomes from the UCSC Genome Browser: human (hg38, hg19, T2T), mouse (mm39, mm10), rat, zebrafish, fly, worm and yeast. Each genome opens with its genes and its chromosome bands. **Use offline** keeps a genome on your computer.
- Sequences: FASTA (also bgzip) and 2bit. Features: BED, GFF3, GTF and bigBed. Reads: BAM. Variants: VCF. Signal: bigWig and bedGraph.
- Files on your computer and on web servers. genie reads only the parts of a file that the view needs.
- A list of public UCSC tracks for each genome, for example MANE, GENCODE, ClinVar, dbSNP, phyloP and ENCODE cCREs.

### Tracks

- Put the pointer on a feature, a read or a variant to see its data and the fields of its file. Click a field to filter the track or to label the features with it.
- Reads: coverage, mismatches, colors for unusual pairs, sorting and grouping by phase or strand, the other parts of split reads, and a light color for reads that are placed in more than one spot.
- RNA-seq: an arc for each splice junction, with its number of reads and its strand.
- Long reads: colors for base modifications such as 5mC and 5hmC, from the MM and ML tags.
- Variants: sites in the colors of their bases and a row of genotypes for each sample.
- Signal: bars or lines, a fixed or shared range, and two or more tracks in one lane.

### Navigation

- Type a locus, a gene name or a variant ID. Go back and forward through your jumps.
- Keep regions in a list, and import or export them as BED files.
- Show two to four loci side by side.

### Images and automation

- Save the view as a PNG or an SVG image. Copy the locus, the sequence or a card.
- Save and open sessions. genie keeps your last session.
- `genie snapshot` makes one image for each region of a BED file, without a window.
- Other programs can control genie with IGV batch commands on port 60151.

### Packages

- A Windows installer, a macOS disk image for Apple silicon and Intel, and a Linux AppImage and .deb package.
- The packages are not signed. The README tells how to open genie the first time.
