Standalone ARM scanner experiment, preceding full-runtime validation.
The new hit handling replaces a source-level 16-byte mask store and scalar
search with two register-word extracts and trailing-zero counts. LLVM already
eliminated that store in the original binary; it was not an actual spill.
The no-hit
loop is unchanged. The standalone module uses to_le(); production enables
this replacement only on little-endian ARM and retains the original path on
big-endian ARM. Non-ARM production scanners are unchanged.

Built with rustc --edition 2021 -O experiment.rs --emit asm,link -o experiment.
The executable checks 38,207,488 cases covering every byte value, lengths 0..96,
alignments 0..31, all byte positions, and all 65,536 hit masks. Each contract is
compared to a scalar oracle. Validation completed successfully.

The exported old_scan wrapper contains 229 emitted instructions (916 bytes),
versus 176 (704 bytes) for new_scan. These standalone code sizes are diagnostic;
they do not establish a whole-runtime speedup or explain the numeric regression
by themselves. experiment.s retains the generated assembly for review.

The later full-runtime measurements rejected the experiment: escaped parsing
regressed 50.63% and record-array parsing regressed 5.89–9.51%. See ../README.md.
