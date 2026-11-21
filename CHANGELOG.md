# Changelog

## Unreleased

### New tracks

- genie opens splice junction files as junction tracks: the `SJ.out.tab` files of STAR, and the junction BED files of TopHat and regtools. Each junction is an arc in the color of its strand, higher and thicker for more reads, and its card shows the fields of the file, such as the multi-mapping reads and the splice motif.
- **View as pairs** in the menu of a read track puts the two mates of each pair on one row, with a line between them.
- **View mate** in the open card of a read shows the read and its mate side by side, also when the mate is on another chromosome.
- **Methylation difference** in the menu of a read track in groups compares the first two groups at each site, for example haplotype 1 against haplotype 2: a bar up or down in percentage points, and a dot where Fisher's exact test gives p < 0.05.
- **DMR table** in the menu of a track with a methylation difference lists the DMRs in view: runs of 3 or more CpGs that differ in the same direction with p < 0.05, with their length, mean difference and smallest p. One CpG that misses does not split a DMR. Click a row to go there, or export the DMRs as a BED file.
- A bedMethyl track that compares with another bedMethyl track can show their **Methylation difference**. A track in a folder of the tracks panel stands for all the bedMethyl tracks of the folder, with their counts added at each site, for example tumor samples against normal samples.
- An insertion of 10 bases or more in a read, as in long reads, shows its length in a purple box in every view. The read card of an insertion under the pointer shows its length, its place and its bases.
- **Isoforms** in the menu of a read track puts long RNA reads with the same splice junctions, in 5 bases or less, in one row: the exons of the isoform, its number of reads, and the name and ID of the transcript with the same introns in a feature track in view, or the gene of the transcript with the most of its junctions and "novel", as "HNRNPA1 novel 1". A read that is a part of a longer isoform counts for it, and the ends of the junctions first move to the introns of the transcripts in 20 bases or less. A row needs 2 reads or more and 1 % or more of the spliced reads in view. The line under the rows counts the other isoforms and the reads without a splice junction.
- **Isoform table** in the menu of a read track lists the isoforms in view under the tracks: the name, the kind as the classes of SQANTI (known, part of a known, new combination, new junction), the place, the reads and their share, the exons and the length. **Show all** adds the isoforms with fewer reads. With groups of reads, as **Group by phase**, a column for each group gives its reads and their share, and for two groups the p of Fisher's exact test finds the isoforms that one haplotype uses more. Click a row to go there, click the name of a column to sort, and **Export…** saves BED12, GTF or TSV.
- genie opens GenBank and EMBL files: the sequence becomes the reference and the features a track, or only the track opens when the reference in view has the same records. Genes show as their transcripts with the CDS that fits them, records take the names of NCBI (accession and version), and features keep the colors of SnapGene, ApE, Benchling and Artemis.
- **Design primers…** in the ruler and region menus and **Primers** in the open card of a variant design PCR primers around the target, as Primer3 does: Tm by nearest neighbors, GC, 3′ ends and complementarity, off the variants of open tracks. genie checks each pair for other products up to 4 kb in the whole genome of a local reference, and shows the 5 best pairs.
- **B-allele frequency** in the menu of a variant track shows a lane with the fraction of the reads of one sample that carry an allele that is not the reference allele, from AD or else VAF, AF or BAF, at the sites with a call and 10 reads or more. The lane also shows under the density of a wide view, up to a whole chromosome, so that a gain or a loss of heterozygosity is easy to see.
- **Review variants…** in the menu of a variant track opens a bar that goes from variant to variant with N and P, and marks each one true, false or not sure with T, F and U, with a note. The marks show in a strip over the sites, in the cards and in the variant table, and filters and **Next variant** can use them. Sessions and undo keep them, and **Export…** saves a TSV file or a copy of the VCF file with REVIEW and REVIEW_NOTE.
- **LD** in the menu of a variant track shows the r² of the pairs of variants in view in a triangle under the sites, as Haploview does, from the genotypes of all samples or of a group of the samples, such as EUR. While a track shows its LD, the dots of GWAS tracks take the LocusZoom colors of their r² with the top dot, or with a dot that **Lead** in its card picks.
- **Phase by** in the menu of a read track gives the reads without an HP tag haplotype 1 or 2 from the phased SNVs of a sample of a variant track: the bases of a read with a quality of 20 or more vote in each phase set, and a read needs 2 votes or more and twice the votes of the other haplotype. genie uses the sample with the name of the reads (SM), else the first. Then **Group by phase**, **Methylation difference** and the **DMR table** work for reads without HP tags. Sessions keep the variant track and the sample.
- genie opens motif files of MEME, JASPAR and HOMER. A dialog finds motifs by name or ID, and the track shows the sites of the picked motifs on the two strands with an exact p-value under 0.0001, as FIMO finds them. The track menu changes the motifs and the threshold.
- **Reference tracks** in the sequence menu, the genie menu and the palette make tracks from the bases of the reference, without a file: **GC %** in windows of 5 to 1,000 bases, **CpG islands** as UCSC finds them, **Repeats** from the soft-masked bases, and **Restriction sites…** of 60 common enzymes or typed sites, with the fragment lengths between the cuts in the cards.
- genie opens cooler files (`.cool` and `.mcool`) as Hi-C tracks, with their balancing weights as normalizations. It has its own reader of the HDF5 files that cooler writes.
- **Virtual 4C from here** in the menu of a Hi-C track shows the contacts of one place with each bin in view, in a lane under the map.
- A signal track can compare its values with another signal track: **Compare with** in the track menu shows the log2 ratio, the ratio or the difference of the two tracks at each pixel, for example ChIP over input.
- **BLAT a sequence…** sends bases, a read or the clipped part of a read to the BLAT server of UCSC with your UCSC API key. The alignments show in a list and as a track, and genie opens other PSL files too. Without a key, the dialog opens the same search on the UCSC page in your web browser.
- genie opens PAF files from minimap2, wfmash and other aligners. Each alignment shows on the reference in view with the name of its query, in blocks between the long deletions of its CIGAR. Its card shows the place on the query, the identity, MAPQ and the tags. genie sorts and indexes large PAF files in its cache.
- **Dot plot** in the menu of a PAF track shows its alignments in place of the tracks: the reference across, the queries down, a blue line for each forward alignment and a vermillion line for each reverse alignment. Drag a box to zoom in, put the pointer on a line to see its card, and click it to show the alignment in the tracks. Back returns to the plot. **Color by identity** colors the lines from light to dark.
- **Ask gnomAD** in the open card of a variant on hg38 or hg19 shows its allele frequency in gnomAD (v4.1 or v2.1.1) and the frequency of each genetic ancestry group, from the gnomAD API. genie asks only after you turn on **gnomAD** (also in Settings), only for the cards that you keep open, and at most 8 times a minute.
- **Synteny view** in the bar of the dot plot shows the targets in a row at the top and the queries in a row at the bottom, with a band between the two parts of each alignment. An inversion shows as a band that crosses over itself. Drag along a row to zoom in, and the other row shows the parts that align.
- A family variant track checks the calls of each child against its parents, from the father and the mother in the sample table (for example a PLINK `.ped` file). The field Inheritance tells the Mendelian errors and the de novo variants, a frame shows around the call of the child, and the track menu has **Only de novo variants** and **Only Mendelian errors**. genie checks X only for daughters.

### Tracks panel

- Ctrl+Shift+T or the list button shows the tracks panel at the left of the tracks: a row for each track with an eye that hides or shows it, a field that finds tracks by name, and the track menu on a right-click. A hidden track does not show and genie does not read its data. **Hide** in the track menu also hides a track.
- Folders group tracks in the panel: **Folder** in the track menu puts a track in a folder or makes a new one. A folder folds its rows, and its eye hides or shows all its tracks. Sessions keep hidden tracks and folders.
- Drag a row of the panel to change the order of the tracks, or of the folders. The panel shows the new order while you drag.

### Undo

- Ctrl+Z (Cmd+Z on macOS) undoes the last work on tracks and regions, and Ctrl+Y or Ctrl+Shift+Z does it again, for up to 100 steps. Moves of the view stay with Back and Forward.

### Genomes

- **Other NCBI assemblies…** in the start view and **NCBI assembly…** in the genome menu find an assembly of UCSC GenArk by its species, its common name or its accession, then open its hub and its genome. Open URL and Open a track hub also take an accession such as `GCF_016699485.2`. Outdated assemblies come last.
- The genome of an assembly hub opens with the contig sizes of its `chromSizes` file, so a 2bit file on the web needs a few requests, not one for each contig. Sessions keep the sizes file.

### Files on the web

- genie sends at most 4 requests at one time to one server and reads near parts of a file in one request, so that servers do not take it for an attack. A busy server (HTTP 429 or 503) gets the request again after the wait that it asks for.
- Messages and logs do not show the keys, tokens and signatures in web addresses.

### Images

- Ctrl+E opens the export dialog. It gives the image a title, a scale bar and the regions in view as grey bands with their names. The session keeps the title and the settings.
- `genie snapshot` gives each image the name of its region as the title.

### Website

- A tutorial shows the main work in ten short clips of genie with real data: open a genome, go to a gene, compare loci, add tracks, move and zoom, open reads from the web, read cards, change a track, find a sequence and save the work.
- The manual has the full guide on pages with a search box. Other pages list all keys and mouse actions, the changes of each version, example sessions with public data, and answers to common questions.
- The home page shows the features of 0.3 with real data, and the download button offers the package for your system.
- The pages load less: the fonts are about ten times smaller, and the clips play only while they are on the screen.

### Fixes

- A menu that is taller than the window scrolls. Before, the items at its bottom, such as **Remove**, could be out of the window.
- **View mate** of a spliced read, as in RNA-seq, shows the start of the read and the start of its mate. Before, genie used the length of the read with its introns, so the panels could show an intron and miss the mate.
- A tall read track stops at the top of the variant table or the DMR table. Before, its reads could show over the table.
- In a narrow window, a locus box with several loci keeps the window buttons in the window. Before, the buttons could go out of view.
- In the narrowest window, the title bar hides the buttons that the genie menu also has: save, export, help, settings and the whole genome. Before, the bar was wider than the window, and the buttons at its right were out of view.
- A Hi-C map of a whole chromosome is bright. When the contacts of near bins are much higher than the others, the colors follow a curve that gives the middle count a fifth of the full color. Before, only the diagonal had color. Views of 10 Mb and less keep almost the same colors.
- genie reads CRAM files with empty blocks that name a codec, as the 1000 Genomes files for GRCh38 have. Before, they gave the message "A CRAM block is not valid".
- The track menus of interaction, Hi-C, copy number and table tracks have the correct height. Before, genie made space for items that these menus do not have, so a menu near the bottom of the window could move up too far.
- In a bigBed card, each item of a list keeps its place, and an item without a value (empty, `-inf` or `nan`) shows as –. Before, the card did not show empty items, so the alleles of dbSNP did not agree with the frequencies, and the card showed `-inf`.
- A bigBed card shows the term of a Sequence Ontology number, for example `5_prime_UTR_variant (SO:0001623)` for 1623 in dbSNP. The card does not show 0, which is no term.
- An SVG image of the whole genome gives each chromosome its width, as the view does. Before, all chromosomes had the same width. Images of the whole genome show the name of each chromosome, as the view does, and not a ruler.

## 0.3.1

The check for updates works again.

### Fixes

- genie reads the answer of GitHub also when it is long. Before, the check for updates said that the answer "is not valid".

## 0.3.0

genie shows methylation, structural variants, Hi-C maps, Sashimi plots and GWAS results. It opens CRAM, BCF, track hubs and files in cloud buckets, and it lifts tracks to another assembly.

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

### Primer pairs

- **Primers** in the find bar finds the PCR products of a forward and a reverse primer on the two strands, up to 4,000 bp long. The 5 bases at the 3′ end of each primer must match.
- In the lane, a product is a line with a block for each primer. Enter goes to the next product, and its card shows its length, its strand, its mismatches and its primers.

### Sashimi plots

- **Sashimi plot** in the menu of a read track shows its coverage as an area and its splice junctions as arcs above and below the area, each with its number of reads.
- **Arcs ≥** hides the junctions with few reads. **Shared scale** gives the Sashimi plots of several samples one range. Export image saves the plots as PNG or SVG.

### Variant table

- **Variant table** in the menu of a variant track shows its variants in view in a table under the tracks. Each field is a column: the alleles, the counts of the calls, Quality, Filter, the annotations and the INFO fields.
- Click a column name to sort the rows, type to find rows, and click a row to go to its variant. **Export TSV** saves the rows.

### Liftover

- **Lift over…** in the track menu moves the data of a track from a different assembly to the assembly in view with a UCSC chain file, for example `hg19ToHg38.over.chain.gz`. It works for feature, variant, signal, interaction, copy number, table and GWAS tracks.
- Features keep their exons and coding parts. On a chain to the opposite strand, features change strand and variants show the reverse complement of their alleles.
- The locus box finds the names of a lifted track at their new places. Sessions keep the chain file of each track. **Remove lift** shows the data at its own places again.
- **Lift…** under the region list moves the regions from a different assembly to the assembly in view.

### GWAS tracks

- genie opens GWAS summary statistics as Manhattan plots: of the GWAS Catalog, PLINK 2, REGENIE, SAIGE and BOLT-LMM. genie knows them by their header line or by their names (`.glm.*`, `.regenie`).
- A dot shows the -log10 p of each variant. A line marks 5×10⁻⁸, and the dots over it are vermillion. The whole-genome view shows all chromosomes, with a lighter color for every second one.
- The card of a dot shows its row, for example its rsID, alleles, beta and p-value.
- genie reads only the part in view of a file with a tabix index, and all of a file without an index up to 1 GB.

### Table tracks

- genie opens tab-separated tables with a tabix index as table tracks, for example the CADD scores. The header line names the columns.
- A table track shows a column of numbers as a signal, at first the last one. The track menu selects a different column, or shows the rows as features with all their columns in the card. Sessions keep the choice.

### BCF files

- genie opens BCF files, the binary VCF files of bcftools, with their CSI index, from your computer and from the web. They have all the features of VCF tracks: genotypes, cards, filters, annotations, structural variants, densities from the index and sample groups.
- A BCF file without an index opens if it is 50 MB or less.

### Hi-C contact maps

- genie opens the `.hic` files of Juicer (versions 6 to 9) as Hi-C tracks, also from the web and from track hubs (type `hic`). The contacts show as a triangle over the diagonal, from light grey to vermillion.
- The size of the bins follows the zoom. genie reads only the blocks of the matrix that the view needs.
- The track menu selects the normalization of the file (for example KR or SCALE, or none) and observed/expected ratios in blue and vermillion. Sessions keep both.
- The card of a contact shows its two bins, their distance, the value and the normalization.

### Structural variants

- Structural variants show over their full length in the colors of their type: dark grey for deletions, vermillion for duplications, green for inversions and purple for insertions. genie finds them by SVTYPE, by symbolic alleles such as `<DEL>`, and by breakends.
- Lines with ends show the confidence intervals of the start and the end (CIPOS and CIEND).
- Breakends (BND) are arcs to their other ends in a lane above the sites, or stubs when the other end is on another chromosome.
- genie finds the other end of a breakend in its alternate allele, or in CHR2 with POS2, END2 or END, as gnomAD-SV and other callers write it. A breakend takes one base, also when its END is on the other side. CTX translocations are breakends too.
- Complex rearrangements, such as the CPX records of gnomAD-SV, show over their full length. Their card says "Complex rearrangement".
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
- genie checks the CRC of each block of a file compressed with bgzip. A damaged block gives an error. Before, a damaged BAM file could give wrong reads, and genie could stop for minutes on a read with a very long CIGAR.
- genie rejects a BAM read whose CIGAR does not fit the length of its bases, as htslib does.
- A test gives cut and damaged copies of all test files to all readers. Each reader must give an error or data, and must not stop or hang.
- A file that stops before the end of its data gives the message "The file stops before the end of its data. Make sure that the file is complete." Before, the message was "failed to fill whole buffer".
- The card of a variant with no ID shows its change once, as its title. Before, the change was also the first line of the card.

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
