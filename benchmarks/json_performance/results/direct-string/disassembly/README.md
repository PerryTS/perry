# Tape-builder disassembly comparison

`llvm-objdump --disassemble-symbols` output for the Unicode reference, primitive-array checkpoint and direct-output candidate. Symbols and worker hashes are recorded in the associated provenance records. The corrected diffs normalize absolute code addresses (hex values of at least eight digits), preserving instruction operands and symbol offsets.

All three tape builders contain 1,348 instructions. Differences after address normalization are confined to cold panic metadata at the end. The core decimal parser contains 284 instructions and its normalized sequence is identical. This evidence excludes added hot instructions in these functions as an explanation for the numeric-parse slowdown; it does not prove the exact cache or predictor behavior responsible.

Dump indentation is normalized to spaces for the repository whitespace check; instruction text and addresses are preserved.
