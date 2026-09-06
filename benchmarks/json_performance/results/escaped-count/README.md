Measured source 4b9d577d4e81518f83a8b4a1e4f319d328597f74.

Valid UTF-8 escaped strings write directly into final output with native scalar
plans and rooted input rederivation. ARM expansion counts now use bounded
16-byte table lookups/reductions; tails and other architectures use a scalar
lookup table. The maximum block sum is 80, so byte reductions cannot overflow.
Malformed UTF-8/WTF-8 still declines to the general writer. No production GC
policy or parse-boundary interaction changes.

172 JSON runtime tests, 38 compiled Node comparisons and 34 moving-GC runs
pass. All 59 source hashes and matched compiler/build settings are verified.
The full matrix qualifies with 32 clean observations: 152 verifications,
456 timing trials and 432 memory trials. Targets are 12/38 CPU, 58/74 peak RSS
and 28/36 retained RSS medians at/below the better Node/Bun median. The
644-trial paired matrix plus 24 retained-empty trials qualifies with 38 clean
observations. All-row and no-regression acceptance remain false.

Escaped stringify CPU is 0.953 ms versus preceding empty-parse 1.901,
Node 1.898 and Bun 2.087 ms. Its peak RSS is 57.22 MiB versus matching Perry
reference 58.06, Node 124.59 and Bun 74.75 MiB. Call counts are identical across
arms within each row, and change with calibration between matrices. Compare
paired same-call trials before attributing RSS changes between experiments.
The paired escaped stringify comparison improves 49.97% versus empty-parse
(1.8997 to 0.9504 ms), 38.93% versus the first direct-output worker, and 48.80%
versus tape-owned, with separated ranges. Peak RSS falls 0.84375 MiB versus
empty-parse, but rises 0.09375 MiB versus the first direct-output worker at
the same 169 calls. Paired wide/16 KB array/heterogeneous stringify improve
1.99%/1.74%/1.01% versus empty-parse.

Confirmed paired regressions versus empty-parse: small-record stringify
+1.60%, empty/tiny/small/1 KB object parse +2.44%/+0.95%/+0.56%/+0.58%,
and tiny-object stringify +0.89%. The full small-record median was +2.46%;
both sets and their observed ranges are preserved. Eight-MB array stringify
peak RSS rises 0.046875 MiB. The older eight-MB array-parse peak regression
remains +11.359375 MiB versus parse-entry. Retaining 200k empty objects uses
24.64 MiB versus reference 24.86, Node 83.44 and Bun 50.20 MiB; these extra
trials do not change the original target inventory. All prior unresolved
regressions remain requirements.

Local instruction counts fall 31.54% for escaped stringify, but rise 2.83%
for small-record stringify. Disassembly confirms vector count emission and
the enlarged shared emitter; see next-steps.md for a proposed recovery.

Array-root parsing may defer materialization. Stringify starts fully
materialized. CPU includes default GC; RSS is whole-process memory.
