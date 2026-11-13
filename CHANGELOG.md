# Changelog

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
