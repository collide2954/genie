#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C
export PATH="$HOME/bin:$PATH"
cd "$(dirname "$0")"

awk -v OFS='\t' '
function draw(limit) { seed = (seed * 16807) % 2147483647; return int(seed / 2147483647 * limit) }
BEGIN {
    seed = 42
    split("chr1 chr2 chrX", contigs, " ")
    for (c = 1; c <= 3; c++) {
        start = 1000
        for (i = 1; i <= 900; i++) {
            start += 50 + draw(1200)
            size = i % 97 == 0 ? 50000 + draw(250000) : 60 + draw(4000)
            name = sprintf("%s_%03d", contigs[c], i)
            strand = draw(2) ? "+" : "-"
            if (i % 3) {
                print contigs[c], start, start + size, name, 0, strand
                continue
            }
            count = 2 + draw(3)
            block = int(size / (2 * count))
            step = int(size / count)
            sizes = ""
            starts = ""
            for (b = 0; b < count; b++) {
                sizes = sizes block ","
                starts = starts (b < count - 1 ? b * step : size - block) ","
            }
            print contigs[c], start, start + size, name, 0, strand, start + 10, start + size - 10, 0, count, sizes, starts
        }
    }
}' | sort -k1,1 -k2,2n > genes.bed.tmp
{ printf '#chrom\tstart\tend\tname\n'; cat genes.bed.tmp; } | bgzip > genes.bed.gz
awk -v OFS='\t' '
{
    color = NR % 5 == 0 ? "0" : $6 == "+" ? "200,40,40" : "40,40,200"
    if (NF == 6) print $1, $2, $3, $4, $5, $6, $2, $3, color, 1, ($3 - $2) ",", "0,"
    else { $9 = color; print }
}' genes.bed.tmp > genes.bb.tmp
printf 'chr1\t3000000\nchr2\t3000000\nchrX\t3000000\n' > genes.sizes
bedToBigBed -type=bed12 -extraIndex=name -blockSize=4 -itemsPerSlot=8 genes.bb.tmp genes.sizes genes.bb > /dev/null
rm genes.bed.tmp genes.bb.tmp genes.sizes
tabix -f -p bed genes.bed.gz
{
    for region in chr1:0-20000 chr1:350000-420000 chr2:300000-301000 chrX chr9:0-100; do
        chrom=${region%%:*}
        printf '>records %s\n' "$region"
        if [ "$chrom" = "$region" ]; then
            bigBedToBed -chrom="$chrom" genes.bb stdout 2> /dev/null || true
        else
            range=${region#*:}
            bigBedToBed -chrom="$chrom" -start="${range%-*}" -end="${range#*-}" genes.bb stdout \
                2> /dev/null || true
        fi
    done
    for name in chr1_003 chr2_450 chrX_900 missing; do
        printf '>name %s\n' "$name"
        bigBedNamedItems genes.bb "$name" stdout 2> /dev/null || true
    done
    for query in "chr1 0 3000000 50" "chr2 100000 200000 20" "chrX 0 600000 7"; do
        for kind in mean coverage; do
            printf '>%s %s\n' "$kind" "$query"
            bigBedSummary -type="$kind" genes.bb $query 2> /dev/null || echo none
        done
    done
} > genes.bb.expected

cat > genes.pred.as << 'AS'
table bigGenePred
"bigGenePred gene models"
   (
   string chrom;       "Reference sequence chromosome or scaffold"
   uint   chromStart;  "Start position in chromosome"
   uint   chromEnd;    "End position in chromosome"
   string name;        "Name or ID of item"
   uint score;         "Score (0-1000)"
   char[1] strand;     "+ or - for strand"
   uint thickStart;    "Start of where display should be thick (start codon)"
   uint thickEnd;      "End of where display should be thick (stop codon)"
   uint reserved;      "RGB value (use R,G,B string in input file)"
   int blockCount;     "Number of blocks"
   int[blockCount] blockSizes; "Comma separated list of block sizes"
   int[blockCount] chromStarts; "Start positions relative to chromStart"
   string name2;       "Alternative/human readable name"
   string cdsStartStat; "Status of CDS start annotation"
   string cdsEndStat;   "Status of CDS end annotation"
   int[blockCount] exonFrames; "Exon frame {0,1,2}, or -1 if no frame for exon"
   string type;        "Transcript type"
   string geneName;    "Primary identifier for gene"
   string geneName2;   "Alternative/human readable gene name"
   string geneType;    "Gene type"
   )
AS
printf '%s\n' \
    'chr1	1000	5000	ENST01.1	0	+	1100	4900	0	2	1000,1000,	0,3000,	GENEA	cmpl	cmpl	0,1,	coding	ENSG01	GENEA	protein_coding' \
    'chr1	2000	2600	ENST02.3	0	-	2000	2000	0	1	600,	0,	GENEB	none	none	-1,	noncoding	ENSG02	GENEB	lncRNA' \
    'chr1	7000	9000	ENST03.2	0	+	7100	8900	0	1	2000,	0,	GENEC	cmpl	cmpl	0,	coding	ENSG03		protein_coding' \
    > genes.pred.tmp
printf 'chr1\t20000\n' > genes.pred.sizes
bedToBigBed -tab -type=bed12+8 -as=genes.pred.as genes.pred.tmp genes.pred.sizes genes.pred.bb > /dev/null
rm genes.pred.as genes.pred.tmp genes.pred.sizes

cat > snps.as << 'AS'
table snps
"Variants: BED 4 plus extra fields, like bigDbSnp"
   (
   string chrom;       "Reference sequence chromosome or scaffold"
   uint   chromStart;  "Start position in chromosome"
   uint   chromEnd;    "End position in chromosome"
   string name;        "Variant identifier"
   lstring ref;        "Reference allele"
   int altCount;       "Number of alternate alleles"
   lstring alts;       "Alternate alleles"
   )
AS
printf '%s\n' \
    'chr1	100	101	rs1	A	1	G,' \
    'chr1	200	201	rs2	C	2	T,G,' \
    'chr1	300	302	rs3	AT	1	A,' \
    > snps.tmp
cat > ccres.as << 'AS'
table ccres
"Regulatory elements: BED 9 plus extra fields, like ENCODE cCREs"
   (
   string chrom;       "Reference sequence chromosome or scaffold"
   uint   chromStart;  "Start position in chromosome"
   uint   chromEnd;    "End position in chromosome"
   string name;        "Name of item"
   uint score;         "Score (0-1000)"
   char[1] strand;     "+, - or . for unknown"
   uint thickStart;    "Start of the thick part"
   uint thickEnd;      "End of the thick part"
   uint reserved;      "RGB value (use R,G,B string in input file)"
   string ccre;        "Class"
   float zScore;       "Highest DNase Z-score"
   string ucscLabel;   "Short class label"
   lstring _mouseOver; "Mouse over text"
   )
AS
printf '%s\n' \
    'chr1	100	400	EH1	0	.	100	400	255,167,0	pELS	2.5	enhP	<b>EH1</b>' \
    'chr1	500	900	EH2	0	.	500	900	0,176,240	CTCF-only	1.25	7|CTCF	<b>EH2</b>' \
    > ccres.tmp
printf 'chr1\t20000\n' > extra.sizes
bedToBigBed -tab -type=bed4+3 -as=snps.as -extraIndex=name snps.tmp extra.sizes snps.bb > /dev/null
bedToBigBed -tab -type=bed9+4 -as=ccres.as ccres.tmp extra.sizes ccres.bb > /dev/null
rm snps.as snps.tmp ccres.as ccres.tmp extra.sizes

cat > gencode.pred.as << 'AS'
table gencodeBGP
"GENCODE bigGenePred"
   (
   string chrom;       "Reference sequence chromosome or scaffold"
   uint   chromStart;  "Start position in chromosome"
   uint   chromEnd;    "End position in chromosome"
   string name;        "Ensembl ID"
   uint score;         "Score (0-1000)"
   char[1] strand;     "+ or - for strand"
   uint thickStart;    "Start of where display should be thick (start codon)"
   uint thickEnd;      "End of where display should be thick (stop codon)"
   uint reserved;      "RGB value (use R,G,B string in input file)"
   int blockCount;     "Number of blocks"
   int[blockCount] blockSizes; "Comma separated list of block sizes"
   int[blockCount] chromStarts; "Start positions relative to chromStart"
   string name2;       "UCSC Genes ID"
   string cdsStartStat; "Status of CDS start annotation"
   string cdsEndStat;   "Status of CDS end annotation"
   int[blockCount] exonFrames; "Exon frame {0,1,2}, or -1 if no frame for exon"
   string type;        "Transcript type"
   string geneName;    "Gene Symbol"
   string geneName2;   "UniProt display ID"
   string geneType;    "Gene type"
   )
AS
printf '%s\n' \
    'chr1	1000	5000	ENST01.1	0	+	1100	4900	0	2	1000,1000,	0,3000,	uc001aaa.1	cmpl	cmpl	0,1,	none	GENEA	P04406	protein_coding' \
    'chr1	2000	2600	ENST02.3	0	-	2000	2000	0	1	600,	0,	uc001aab.1	none	none	-1,	none	GENEB	none	lncRNA' \
    'chr1	7000	9000	ENST03.2	0	+	7100	8900	0	1	2000,	0,	uc001aac.1	cmpl	cmpl	0,	none	none	Q15021	protein_coding' \
    > gencode.pred.tmp
printf 'chr1\t20000\n' > gencode.pred.sizes
bedToBigBed -tab -type=bed12+8 -as=gencode.pred.as gencode.pred.tmp gencode.pred.sizes gencode.pred.bb > /dev/null
rm gencode.pred.as gencode.pred.tmp gencode.pred.sizes

awk -v OFS='\t' '
function draw(limit) { seed = (seed * 16807) % 2147483647; return int(seed / 2147483647 * limit) }
function exons(contig, strand, id, position, count, exon, intron, only_ends, gtf,    e, left, low, high) {
    for (e = 0; e < count; e++) {
        if (only_ends && e > 0 && e < count - 1) continue
        left = position + e * (exon + intron)
        print contig, "genie", "exon", left + 1, left + exon, ".", strand, ".", "Parent=" id
        print contig, "genie", "exon", left + 1, left + exon, ".", strand, ".", gtf > "genes.gtf.tmp"
        low = e == 0 ? left + 30 : left
        high = e == count - 1 ? left + exon - 30 : left + exon
        print contig, "genie", "CDS", low + 1, high, ".", strand, "0", "Parent=" id
        print contig, "genie", "CDS", low + 1, high, ".", strand, "0", gtf > "genes.gtf.tmp"
    }
}
function ids(gene, transcript, name) {
    return "gene_id \"" gene "\"; transcript_id \"" transcript "\"; gene_name \"" name "\";"
}
BEGIN {
    seed = 7
    split("chr1 chr2", contigs, " ")
    for (c = 1; c <= 2; c++) {
        position = 5000
        for (g = 1; g <= 120; g++) {
            position += 2000 + draw(20000)
            count = 2 + draw(6)
            exon = 100 + draw(400)
            intron = 500 + draw(3000)
            span = count * exon + (count - 1) * intron
            strand = draw(2) ? "+" : "-"
            gene = sprintf("%s.g%03d", contigs[c], g)
            name = sprintf("GENE%s%03d", substr(contigs[c], 4), g)
            print contigs[c], "genie", "gene", position + 1, position + span, ".", strand, ".", "ID=" gene ";Name=" name
            print contigs[c], "genie", "gene", position + 1, position + span, ".", strand, ".", "gene_id \"" gene "\"; gene_name \"" name "\";" > "genes.gtf.tmp"
            print contigs[c], "genie", "mRNA", position + 1, position + span, ".", strand, ".", "ID=" gene ".t1;Parent=" gene ";Name=" name "-201"
            print contigs[c], "genie", "transcript", position + 1, position + span, ".", strand, ".", ids(gene, gene ".t1", name) " transcript_name \"" name "-201\";" > "genes.gtf.tmp"
            exons(contigs[c], strand, gene ".t1", position, count, exon, intron, 0, ids(gene, gene ".t1", name) " exon_number 1;")
            if (g % 10 == 0) {
                print contigs[c], "genie", "mRNA", position + 1, position + span, ".", strand, ".", "ID=" gene ".t2;Parent=" gene ";Name=" name "-202"
                print contigs[c], "genie", "transcript", position + 1, position + span, ".", strand, ".", ids(gene, gene ".t2", name) " transcript_name \"" name "-202\";" > "genes.gtf.tmp"
                exons(contigs[c], strand, gene ".t2", position, count, exon, intron, 1, ids(gene, gene ".t2", name))
            }
            position += span
        }
    }
}' > genes.gff3.tmp
{ printf '##gff-version 3\n'; sort -s -k1,1 -k4,4n genes.gff3.tmp; } | bgzip > genes.gff3.gz
rm genes.gff3.tmp
tabix -f -p gff genes.gff3.gz
sort -s -k1,1 -k4,4n genes.gtf.tmp | bgzip > genes.gtf.gz
rm genes.gtf.tmp
tabix -f -p gff genes.gtf.gz

awk -v OFS='\t' '
function draw(limit) { seed = (seed * 16807) % 2147483647; return int(seed / 2147483647 * limit) }
function bases(count,    text, i) {
    text = ""
    for (i = 0; i < count; i++) text = text substr("ACGT", draw(4) + 1, 1)
    return text
}
function qualities(count,    text, i) {
    text = ""
    for (i = 0; i < count; i++) text = text sprintf("%c", 33 + draw(42))
    return text
}
BEGIN {
    seed = 11
    print "@HD", "VN:1.6", "SO:unsorted"
    print "@SQ", "SN:chr1", "LN:3000000"
    print "@SQ", "SN:chr2", "LN:3000000"
    split("50M 20M2I28M 25M3D25M 15M300N35M 5S45M 3H47M 10M1D10M2I28M 48M2S 20=1X29=", cigars, " ")
    split("50 50 50 50 50 47 50 50 50", lengths, " ")
    split("0 16 0 16 1024 256 512 2048 16", flags, " ")
    for (i = 1; i <= 600; i++) {
        contig = i <= 450 ? "chr1" : "chr2"
        start = 1000 + draw(40000)
        shape = 1 + draw(9)
        flag = flags[1 + draw(9)]
        quality = i % 25 == 0 ? "*" : qualities(lengths[shape])
        print sprintf("read%03d", i), flag, contig, start, draw(61), cigars[shape], "*", 0, 0, bases(lengths[shape]), quality
    }
    for (i = 1; i <= 5; i++) print sprintf("lost%d", i), 4, "*", 0, 0, "*", "*", 0, 0, bases(50), qualities(50)
}' > reads.sam
samtools sort -o reads.bam reads.sam 2> /dev/null
samtools index reads.bam
rm reads.sam

awk -v OFS='\t' '
function draw(limit) { seed = (seed * 16807) % 2147483647; return int(seed / 2147483647 * limit) }
function bases(count,    text, i) {
    text = ""
    for (i = 0; i < count; i++) text = text substr("ACGT", draw(4) + 1, 1)
    return text
}
function tags(    text) {
    text = "NM:i:" draw(4) "\tZF:f:1.5\tZB:B:c,1,-2,3\tZA:A:x"
    if (draw(3) == 0) text = text "\tHP:i:" (1 + draw(2))
    if (draw(2) == 0) text = text "\tRG:Z:group" (draw(2) ? "A" : "B")
    return text "\tXS:i:" draw(300)
}
function mate(name, flag, contig, position, partner, at, insert) {
    print name, flag, contig, position, 60, "50M", partner, at, insert, bases(50), "*", tags()
}
BEGIN {
    seed = 29
    print "@HD", "VN:1.6", "SO:unsorted"
    print "@SQ", "SN:chr1", "LN:100000"
    print "@SQ", "SN:chr2", "LN:100000"
    print "@RG", "ID:groupA", "SM:A"
    print "@RG", "ID:groupB", "SM:B"
    for (i = 1; i <= 300; i++) {
        name = sprintf("pair%03d", i)
        kind = draw(12)
        left = 1001 + draw(90000)
        insert = 250 + draw(150)
        if (kind == 0) insert = 3000 + draw(5000)
        if (kind == 1) insert = 60 + draw(30)
        right = left + insert - 50
        if (kind == 2) {
            mate(name, 65, "chr1", left, "=", right, insert)
            mate(name, 129, "chr1", right, "=", left, -insert)
        } else if (kind == 3) {
            mate(name, 81, "chr1", left, "=", right, insert)
            mate(name, 161, "chr1", right, "=", left, -insert)
        } else if (kind == 4) {
            other = 1001 + draw(90000)
            mate(name, 97, "chr1", left, "chr2", other, 0)
            mate(name, 145, "chr2", other, "chr1", left, 0)
        } else if (kind == 5) {
            mate(name, 73, "chr1", left, "=", left, 0)
        } else {
            mate(name, 99, "chr1", left, "=", right, insert)
            mate(name, 147, "chr1", right, "=", left, -insert)
        }
    }
}' > pairs.sam
samtools sort -o pairs.bam pairs.sam 2> /dev/null
samtools index pairs.bam
rm pairs.sam

awk '
function draw(limit) { seed = (seed * 16807) % 2147483647; return int(seed / 2147483647 * limit) }
BEGIN {
    seed = 3
    split("chr1 chr2 chrM", contigs, " ")
    split("120000 50003 16569", lengths, " ")
    split("60 70 80", widths, " ")
    for (c = 1; c <= 3; c++) {
        print ">" contigs[c] " synthetic"
        line = ""
        for (i = 0; i < lengths[c]; i++) {
            base = substr("ACGT", draw(4) + 1, 1)
            if (int(i / 5000) % 7 == 3) base = tolower(base)
            if (int(i / 1000) % 50 == 17) base = "N"
            line = line base
            if (length(line) == widths[c]) { print line; line = "" }
        }
        if (line != "") print line
    }
}' | bgzip > reference.fa.gz
rm -f reference.fa.gz.fai reference.fa.gz.gzi
samtools faidx reference.fa.gz
samtools faidx reference.fa.gz chr1:1-100 chr1:60000-70000 chr1:119990-120000 chr2 \
    chr2:12345-12345 chrM:16000-16569 > reference.fa.expected

awk -v OFS='\t' '
function draw(limit) { seed = (seed * 16807) % 2147483647; return int(seed / 2147483647 * limit) }
function bases(count,    text, i) {
    text = ""
    for (i = 0; i < count; i++) text = text substr("ACGT", draw(4) + 1, 1)
    return text
}
function genotype(    pick) {
    pick = draw(10)
    return pick < 4 ? "0/0" : pick < 7 ? "0/1" : pick < 9 ? "1/1" : "./."
}
BEGIN {
    seed = 13
    print "##fileformat=VCFv4.2"
    print "##contig=<ID=chr1,length=3000000>"
    print "##contig=<ID=chr2,length=3000000>"
    print "##INFO=<ID=DP,Number=1,Type=Integer,Description=\"Depth\">"
    print "##INFO=<ID=END,Number=1,Type=Integer,Description=\"End\">"
    print "##INFO=<ID=SVTYPE,Number=1,Type=String,Description=\"Type\">"
    print "##FILTER=<ID=LowQual,Description=\"Low quality\">"
    print "##FORMAT=<ID=GT,Number=1,Type=String,Description=\"Genotype\">"
    print "#CHROM", "POS", "ID", "REF", "ALT", "QUAL", "FILTER", "INFO", "FORMAT", "S1", "S2", "S3"
    split("chr1 chr2", contigs, " ")
    for (c = 1; c <= 2; c++) {
        position = 1000
        for (i = 1; i <= 700; i++) {
            position += 1 + draw(400)
            kind = draw(10)
            reference = bases(1)
            info = "DP=" (10 + draw(90))
            if (kind < 6) {
                alternate = substr("ACGT", (index("ACGT", reference) + draw(3)) % 4 + 1, 1)
            } else if (kind < 8) {
                alternate = reference bases(1 + draw(5))
            } else if (kind < 9) {
                reference = reference bases(1 + draw(300))
                alternate = substr(reference, 1, 1)
            } else {
                alternate = "<DEL>"
                info = info ";SVTYPE=DEL;END=" (position + 500 + draw(5000))
            }
            filter = draw(8) ? "PASS" : "LowQual"
            id = draw(3) ? sprintf("rs%d%03d", c, i) : "."
            print contigs[c], position, id, reference, alternate, 10 + draw(90), filter, info, "GT", genotype(), genotype(), genotype()
        }
    }
}' | bgzip > variants.vcf.gz
tabix -f -p vcf variants.vcf.gz
tabix -f -C -p vcf variants.vcf.gz

printf 'chr1\t3000000\nchr2\t1000000\nchrX\t500000\nchrM\t16569\nchrUn_KI270742v1\t186739\n' > signal.sizes
for c in 3 4 5 6 7 8 9; do printf 'chr%s\t10000\n' "$c"; done >> signal.sizes
awk -v OFS='\t' '
function draw(limit) { seed = (seed * 16807) % 2147483647; return int(seed / 2147483647 * limit) }
function value() { return (draw(9000) - 1000) / 8 }
function run(contig, last, count, gap, size,    position, end, i) {
    position = 0
    for (i = 0; i < count; i++) {
        position += draw(4) ? 0 : draw(gap)
        end = position + 1 + draw(size)
        if (end > last) break
        print contig, position, end, value()
        position = end
    }
}
BEGIN {
    seed = 17
    run("chr1", 3000000, 4000, 1500, 600)
    run("chr2", 1000000, 1000, 2000, 2000)
    run("chrX", 500000, 2000, 3, 1)
    run("chrM", 16569, 3, 5000, 100)
    run("chrUn_KI270742v1", 186739, 2, 100, 50)
    for (c = 3; c <= 9; c++) run("chr" c, 10000, 1, 9000, 500)
}' | sort -k1,1 -k2,2n > signal.bedGraph
bedGraphToBigWig -blockSize=4 -itemsPerSlot=64 signal.bedGraph signal.sizes signal.bw
awk '$1 == "chr2"' signal.bedGraph > plain.bedGraph
bedGraphToBigWig -unc plain.bedGraph signal.sizes plain.bw
bgzip -c plain.bedGraph > plain.bedGraph.gz
tabix -f -p bed plain.bedGraph.gz
rm signal.bedGraph plain.bedGraph

awk '
function draw(limit) { seed = (seed * 16807) % 2147483647; return int(seed / 2147483647 * limit) }
BEGIN {
    seed = 19
    print "fixedStep chrom=chr1 start=1001 step=10 span=5"
    for (i = 0; i < 500; i++) print (draw(8000) - 1000) / 8
    print "variableStep chrom=chr1 span=3"
    position = 10001
    for (i = 0; i < 300; i++) { print position, (draw(8000) - 1000) / 8; position += 3 + draw(40) }
    print "fixedStep chrom=chr2 start=100 step=1"
    for (i = 0; i < 1000; i++) print (draw(8000) - 1000) / 8
    print "variableStep chrom=chrX"
    position = 50
    for (i = 0; i < 200; i++) { print position, (draw(8000) - 1000) / 8; position += 1 + draw(2000) }
}' > steps.wig
wigToBigWig -blockSize=4 -itemsPerSlot=64 steps.wig signal.sizes steps.bw
rm steps.wig signal.sizes

signal() {
    local file=$1
    shift
    for query in "$@"; do
        read -r chrom start end bins <<< "$query"
        if [ -z "$bins" ]; then
            printf '>records %s %s %s\n' "$chrom" "$start" "$end"
            bigWigToBedGraph -chrom="$chrom" -start="$start" -end="$end" "$file" stdout
            continue
        fi
        for kind in mean min max; do
            printf '>%s %s %s %s %s\n' "$kind" "$chrom" "$start" "$end" "$bins"
            bigWigSummary -type="$kind" "$file" "$chrom" "$start" "$end" "$bins" 2> /dev/null || echo none
        done
    done > "$file.expected"
}

signal signal.bw "chr1 0 5000" "chr1 1000000 1003000" "chr1 2999000 3000000" "chr2 0 200000" \
    "chrX 100 400" "chrM 0 16569" "chr5 0 10000" "chrUn_KI270742v1 0 186739" "chr9 0 100" \
    "chr1 0 3000000 100" "chr1 0 3000000 7" "chr1 1000000 1010000 50" "chr1 123 777 654" \
    "chr1 5000 5010 20" "chr2 0 1000000 33" "chrX 0 500000 128" "chrM 0 16569 10" "chr9 0 10 5"
signal steps.bw "chr1 990 1100" "chr1 5980 10100" "chr2 500 560" "chrX 0 500000" \
    "chr1 0 20000 40" "chr2 0 2000 2000" "chrX 0 500000 10" "chr1 1000 1010 10"
signal plain.bw "chr2 0 1000000" "chr2 0 1000000 25"

expect() {
    local file=$1
    shift
    for region in "$@"; do
        printf '>%s\n' "$region"
        case "$file" in
        *.bam) samtools view "$file" "$region" 2> /dev/null || true ;;
        *) tabix "$file" "$region" || true ;;
        esac
    done > "${file%.gz}.expected"
}

expect genes.bed.gz chr1:1-1000 chr1:5000-20000 chr1:100000-100500 chr1:350000-420000 \
    chr2:300000-301000 chrX:250000-260000 chrY:1-100 chrX
expect genes.gff3.gz chr1:1-30000 chr1:200000-201000 chr2:500000-600000
expect reads.bam chr1:1-1000 chr1:5000-8000 chr1:20000-24000 chr1:40000-50000 chr2 chrX:1-100
expect pairs.bam chr1 chr2
expect variants.vcf.gz chr1:1-2000 chr1:30000-31000 chr1:60000-60100 chr1:100000-130000 \
    chr2:50000-50010 chr2:90000-95000 chrX:1-100 chr2
