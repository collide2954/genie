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

### Packages

- The Windows installer and the macOS disk image do not ask you to accept the license. The packages contain the license as the file LICENSE.txt.

### Fixes

- A card breaks a word that is too long for one line, for example a long variant name. Before, the word went past the edge of the card.

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
