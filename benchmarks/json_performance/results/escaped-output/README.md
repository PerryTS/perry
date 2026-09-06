Direct escaped output, source bf779ee97f189a0c16f9fa8010188faa13097b0b.

Validated UTF-8 strings plan exact bytes/UTF-16 units and write into the final
managed string for bounded objects, primitive-array fields and heap-string
roots. Plans contain scalar counts only; source pointers are rederived after
allocation. Invalid UTF-8/WTF-8 retains the general path. No GC production
policy, threshold, or parse-boundary-hook changes.

171 JSON runtime tests, 38 compiled Node comparisons and 34 moving-GC runs
pass. A test-only follow-up 9a0a32b5f replaces quadratic Unicode-index hashing
with full UTF-8 byte hashing; runtime code and benchmark worker.o are unchanged.
All 59 effective source hashes and matched build settings are recorded in
provenance.json; the original release stamp and fixture revision are separate.
The initial fixture timeout in the reference worker is preserved and explained.

The full matrix qualifies with 32 clean observations: 152 verifications,
456 timing trials and 432 memory trials. Target medians are 12/38 CPU,
58/74 peak RSS and 28/36 retained RSS. Escaped stringify falls about 18%,
1.910 to 1.570 ms, with peak RSS down 0.9375 MiB; small-record stringify
regresses about 2.5%. The 616-trial paired run plus 24 retained-empty trials
qualifies with 35 clean observations. Paired escaped stringify improves
17.89% versus empty-parse and 16.13% versus tape-owned, with separated ranges.
Escaped peak RSS falls 0.94 MiB versus empty-parse. Small-record stringify
regresses 2.70%, also with separated ranges. Null/inline-string/empty/tiny
parse improve 4.46%/3.84%/2.02%/0.66%; numeric parse improves 1.45%,
wide stringify 2.03%, 16 KB array stringify 0.94%. Other full ranges, memory
differences and older regressions remain in the paired summaries. This is not all-row
or no-regression acceptance.

Local instructions rise 27.78% for escaped stringify despite its measured
CPU improvement. Plan::new disassembly confirms a branchy scalar byte loop.
A following experiment vectorizes only expansion counting.

Array-root parsing may defer materialization; stringify inputs are fully
materialized. CPU includes default GC; RSS is whole-process memory.
