Count-only intermediate candidate, based on `c559a125c`. The exact source is
preserved in [candidate.patch](candidate.patch) (apply with `git apply --unidiff-zero`); source hashes match
[provenance.json](provenance.json). The patch adds only UTF-16 vector counting
and its tests, leaving standard UTF-8 validation in place.

Unicode parse/stringify improve from about 0.98 to 0.79 ms. The profiles put
1,785 of 2,274 parse samples and 1,782 of 2,284 stringify samples in standard
UTF-8 validation. This motivates vectorizing validation as well.

This candidate is **not regression-free**. A seven-process quiet repeat
confirms null stringify increases from 0.0632605 to 0.066990 microseconds.
The main matrix also shows about 4% slower stringify of `"a"`.
All 152 output comparisons, 99 string-related runtime tests, 47 JSON tests,
24 compiled fixtures, and the three GC-stress subjects pass. Full CPU/RSS
parity remains open. See [tables](tables.md), [validation](validation.json),
and [repeat data](recheck-null/timing.jsonl).
