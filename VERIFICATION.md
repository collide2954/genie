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

## Holes

A hole is code or an assumption that has no proof. Each hole is in this list.

### Holes that genie can close

| Hole | State |
|---|---|
| The FASTA offsets of `fai.rs`: the byte of each base. | Open |
| The bins of the BAI, CSI and tabix indexes, and the virtual offsets of BGZF. | Open |
| The places of amino acids on the exons (`protein_blocks`), the length of a coding part, and the codon table. | Open |
| The readers of binary numbers: ITF8 and LTF8 of CRAM, the bases of 2bit files, and the rANS decoders of CRAM. | Open |
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
- The specifications that Verus gets for functions of the Rust standard library, for example `min`, `max`, `saturating_sub` and `saturating_add`.
