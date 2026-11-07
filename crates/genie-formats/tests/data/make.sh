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
rm genes.bed.tmp
tabix -f -p bed genes.bed.gz

awk -v OFS='\t' '
function draw(limit) { seed = (seed * 16807) % 2147483647; return int(seed / 2147483647 * limit) }
function exons(contig, strand, id, position, count, exon, intron, only_ends,    e, left, low, high) {
    for (e = 0; e < count; e++) {
        if (only_ends && e > 0 && e < count - 1) continue
        left = position + e * (exon + intron)
        print contig, "genie", "exon", left + 1, left + exon, ".", strand, ".", "Parent=" id
        low = e == 0 ? left + 30 : left
        high = e == count - 1 ? left + exon - 30 : left + exon
        print contig, "genie", "CDS", low + 1, high, ".", strand, "0", "Parent=" id
    }
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
            print contigs[c], "genie", "mRNA", position + 1, position + span, ".", strand, ".", "ID=" gene ".t1;Parent=" gene ";Name=" name "-201"
            exons(contigs[c], strand, gene ".t1", position, count, exon, intron, 0)
            if (g % 10 == 0) {
                print contigs[c], "genie", "mRNA", position + 1, position + span, ".", strand, ".", "ID=" gene ".t2;Parent=" gene ";Name=" name "-202"
                exons(contigs[c], strand, gene ".t2", position, count, exon, intron, 1)
            }
            position += span
        }
    }
}' > genes.gff3.tmp
{ printf '##gff-version 3\n'; sort -s -k1,1 -k4,4n genes.gff3.tmp; } | bgzip > genes.gff3.gz
rm genes.gff3.tmp
tabix -f -p gff genes.gff3.gz

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
expect variants.vcf.gz chr1:1-2000 chr1:30000-31000 chr1:60000-60100 chr1:100000-130000 \
    chr2:50000-50010 chr2:90000-95000 chrX:1-100 chr2
