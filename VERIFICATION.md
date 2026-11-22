# Verification

This document tells which parts of genie have a formal proof, and which parts have none.

## What a proof is

genie uses the tool [Verus](https://github.com/verus-lang/verus). Verus reads a specification and the Rust code. It proves that the code obeys the specification for all inputs, not only for examples. If Verus cannot prove this, the check fails.

A proof is only as good as its specification. Read the specification to know what a proof shows. The specifications are in the Rust files, in the blocks that start with `verus!`.

genie as a whole is not verified. Only the parts in this document have a proof.

## How to run the proofs

1. Get Verus release 0.2026.11.02.8bf68ee from the [releases of Verus](https://github.com/verus-lang/verus/releases).
2. Put the directory of the release in `~/tools/verus`, or put it in your `PATH`, or give its path in the environment variable `GENIE_VERUS`.
3. Run `powershell -File tools/proofs.ps1` (`pwsh tools/proofs.ps1` on macOS and Linux).

The automatic checks run the proofs on each push. Verus uses Rust 1.99.1, the release that the file `rust-toolchain.toml` gives. The normal build does not use Verus: to the compiler, the code in a `verus!` block is plain Rust without its specification.

## What is proved

| Part | File | What the proof shows |
|---|---|---|
| The regions of a contig | `crates/genie-core/src/region.rs` | The start of each region is never after its end, and no operation has an overflow. `new` refuses a start after the end, and `between` puts two positions in order. The length is the end minus the start. Two regions with positions overlap exactly when a position is in both. The hull has each position of the two regions. The intersection has exactly the positions that are in both regions, and there is no intersection when no position is in both. `clamp_to` keeps exactly the positions before the length. `expand` gives the region of its specification, also at the limits of 64-bit numbers. |
| Complements and codons | `crates/genie-core/src/codon.rs` | The complement of A, C, G and T is T, G, C and A, in the same case, and the complement of the complement of each byte is the byte again. The amino acid of a codon is the entry of the code table at 16 times the value of its first base, plus 4 times the value of its second base, plus the value of its third base, with T, C, A and G as 0 to 3 in upper or lower case: the place is never outside the table. A codon with another byte gives X. |
| The coding bases and the parts of amino acids | `crates/genie-core/src/effect.rs` | The coding length of a transcript, and the number of its exon bases, are the sums of the bases of its parts, or nothing when a sum is more than 64-bit numbers hold. For the bases from `from` to `to` of the coding parts, `slice` gives parts with exactly `to - from` bases in all, and each part is inside a coding part, also when the exons overlap, as a damaged file can give. `protein_blocks` uses `slice` for the 3 bases of each amino acid. |
| The bytes of the bases of a FASTA file | `crates/genie-formats/src/fasta.rs` | The byte of each base is the byte that the index gives: the offset of the record, all the bytes of each line before the line of the base, and the bases before the base on its line. A later base is at a later byte. For a region, genie reads from the byte of its first base to the byte of its last base: the bases of the region and the line ends between them, and no other bytes. An index that gives no bases on a line, lines with fewer bytes than bases, or a byte past the end of 64-bit numbers gives an error. No arithmetic has an overflow, and no division is by zero. |
| The bins of BAI, CSI and tabix indexes | `crates/genie-formats/src/binning.rs` | A binning scheme has 9 levels or fewer under its top bin, and its top bin has 2 to the power of 62 bases or fewer. A scheme for a longer contig gets 9 levels, and the records past its top bin are then too far for the index. The number of the first bin of each level is the number of the bins of the levels above it, with no overflow. The bin of a record is the bin of the deepest level that has its first and its last base, as htslib gives it. On each level, the bins of a query go from the bin of its first base to the bin of its last base, and the bin of each record that overlaps the query is among them. Thus the bins that genie reads for a region include the bin of each record that overlaps the region. |
| The bases of 2bit files | `crates/genie-formats/src/twobit.rs` | Each base of a region is the base of its 2-bit code: four codes in each byte, the first code in the high bits, and T, C, A and G for the codes 0 to 3. genie gives one base for each position of the region, and no place is outside the bytes that it reads. |
| The N and mask blocks of 2bit files | `crates/genie-formats/src/twobit.rs` | The bases of a region that genie changes to N or to lower case are exactly the bases of the region in a block of the file. This is true for blocks in any order, as a damaged file can give, and each part is inside the region. |
| The numbers of CRAM files | `crates/genie-formats/src/cram/bits.rs` | An ITF8 number has 1 to 5 bytes and an LTF8 number 1 to 9 bytes, by the 1 bits at the high end of the first byte. The readers give the value of the bytes by the CRAM format: the value bits of the first byte, then the next bytes with the high bits first, and of the fifth byte of an ITF8 number only its low 4 bits. They give the number of bytes that they read. When the bytes stop before the end of a number, they give nothing, with no panic and no overflow. |
| The bit stream of CRAM files | `crates/genie-formats/src/cram/bits.rs` | The bit at each place is the bit of its byte, the high bit of each byte first. A number of `count` bits has the first bit highest, and it has 32 bits or fewer: for more bits, or when the stream stops before them, the reader gives nothing. The place after the bits is the place of the next bit. |
| The rANS 4x8 decoder of CRAM | `crates/genie-formats/src/cram/rans.rs` | For all input bytes, the decoder of order 0 and of order 1 has no panic and no overflow, and each of its loops comes to an end. A frequency table has a symbol for each of its 4,096 places, frequencies of 4,096 or less, and the first place of the symbol of each place at or before the place. A table that gives a symbol two times, as a damaged file can give, is an error. A size past the limit of 2 to the power of 30 bytes is an error, thus no output has more bytes. The output has the size that the stream gives. |
| The rANS Nx16 decoder of CRAM 3.1 | `crates/genie-formats/src/cram/nx16.rs` | For all input bytes, the decoder of order 0 and of order 1, with the transforms PACK, RLE, CAT and STRIPE, has no panic and no overflow, and each of its loops comes to an end. A stripe can hold stripes to 4 levels. More levels, as a damaged file can give, are an error: thus the decoder calls itself 4 times or fewer, and the stack cannot overflow. Before genie makes an output of a size that the stream gives, it compares the size with the limit of 2 to the power of 30 bytes, thus no output has more bytes. A number of more than 32 bits is an error. A frequency table has a symbol for each of its places, frequencies of its number of places or less, and the first place of the symbol of each place at or before the place. The output of order 0, order 1, PACK and STRIPE has the size that the stream gives. |
| The virtual offsets of BGZF files | `crates/genie-formats/src/bgzf.rs` | A virtual offset is the offset of a block in the file times 65,536, plus a place in the uncompressed block. `pack` makes the virtual offset of a block before 2 to the power of 48 and a place before 65,536, and `unpack` gives the two parts back exactly. A `.gzi` index that gives a block at 2 to the power of 48 or after gives an error, not a virtual offset of a wrong block. |

## Holes

A hole is code or an assumption that has no proof. Each hole is in this list.

### Holes that genie can close

| Hole | State |
|---|---|
| The virtual offsets that the BGZF reader and writer give for their place, in files of 2 to the power of 48 bytes (256 TB) or more. | Open |
| The loop of `chunks` over the levels of an index, the filter of its chunks by the linear index, and the level of a bin number (`span`, `bottom`). | Open |
| Which bases of the coding parts the parts of `slice` are. The reverse strand and the numbers of `protein_blocks`, and the coding parts that `Coding::new` takes from a transcript. | Open |
| The facts of the code table, for example its three stop codons TAA, TAG and TGA. Verus 0.2026.11.02 stops with an internal error when it computes with the bytes of a text constant. | Open, waits for Verus |
| The codings of CRAM that read the bit stream: Huffman, Beta, Gamma and Subexp. | Open |
| The other code that has only whole numbers and bytes, for example the tandem repeats, the walk along a CIGAR, and the rows of features. | Open |

### Holes that stay

Verus cannot work on these parts now, or the parts are not genie's code:

- Floating point numbers, for example the statistics (Fisher's exact test, t-tests), the methylation fractions and the layout of the views. Verus has almost no support for floating point.
- The readers of text formats, for example BED, GFF3, GTF, VCF and the domain files. Verus has little support for the text functions of the standard library.
- Files, the network, the clock and other input and output.
- The window toolkit (iced), the drawing and the rest of the user interface.
- The libraries that genie uses, for example rustls and miniz_oxide.

When Verus supports more of Rust, a part from this list can get a proof. Then it moves to the holes that genie can close.

## What a proof depends on

A proof is correct only if these parts are correct:

- Verus and the Z3 solver that it uses.
- The Rust compiler.
- The specifications that Verus gets for functions of the Rust standard library and of `vstd`, for example `min`, `max`, `saturating_sub`, `saturating_add`, `checked_add`, `checked_mul` and `slice_subrange`.
- The lemmas of the Verus library `vstd` about division, multiplication, powers of two and bits.
- For the bases of a FASTA file: a file with the lines that its index gives.
