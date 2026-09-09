# Corrected PR #10032 validation

The matched 0.5.1528 release build and all 283 JSON tests pass. Every source
hash in `provenance.json` identifies the tree used for these binaries, based
on b2111219c before the correction was committed.

All 14 compiled escaped-record comparisons match Node. The old reference's
results are recorded alongside the corrected results; the before/after witness
must actually differ for the lone-surrogate reproducer. Normal, scheduled
moving/protected and full-GC modes pass that reproducer. Existing retained-leaf,
record-scan and output-debt cadence witnesses also pass (`gc-witness.json`).

The first decoder-only test attempt exposed the generic stringify key defect;
its failed log is preserved as `before-key-fix-tests.log`, not counted as a pass.
`json-tests.log` is the subsequent successful suite including that correction.
These development-host checks establish correctness, not CPU/RSS acceptance.

The final escaped-key fixture in this historical archive has no numeric `id`,
so its scan/sparse RESULT checksum is NaN. Those original checks compare
serialized output. The subsequent
[finite-checksum replay](../decoder-r2-finite-validation/README.md) adds `id` and
checks every numeric field with the accepted R2 and rebuilt-main workers.
