#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C
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
