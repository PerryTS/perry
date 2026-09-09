# Opening-container scan and parse entry experiments

These are follow-ups to the corrected R2 parser in PR #10034. The scanner
candidate is not accepted as regression-free. Global GC policy, construction
allocation and parse-boundary collection scheduling are unchanged.

## R1: wider scan

The ARM64 depth preflight can skip its full quote/depth state machine when no
opening-container byte occurs after the root. R1 retains the initial 16-byte
early-positive probe, then scans 64 bytes per horizontal reduction. Bit 0x20
folds exactly `[` and `{` together; every vector read stays within the slice.
Tests cover all 256 byte values, vector tails, every opening position and
poisoned surrounding bytes. The complete 285-test release JSON suite passed.

Both focused and complete changing-input measurements found gains against
corrected R2: approximately 7.5% on the 1 MB ASCII-string object, 4.5% on Unicode,
and 3.5% on the 1 KB object. Existing Node/Bun CPU and large-container RSS gaps
remain. The complete original suite also caught tiny-input regressions.
Longer original-worker replay confirmed +5.83% on null, +4.44% on the inline
string and +0.81% on the tiny object, with separated sample ranges. R1 therefore
stays experimental. The rotating worker does not reproduce the scalar changes;
these are binary/workload-specific observations, not proof that the vector
scan executes on scalar inputs.

- [Full original parse/stringify/CPU/RSS matrix](results/quiet-opening-scan-r1-all-r5/README.md)
- [Full changing-input suite](results/quiet-opening-scan-r1-rotating-r5/README.md)
- [Longer regression replay](results/quiet-opening-scan-r1-regression-r9/README.md)
- [Focused changing-input evidence](results/quiet-opening-scan-r1-focus-r9/README.md)
- [Correctness, GC witnesses and immutable provenance](results/opening-scan-r1-validation/README.md)

## R2: isolate the empty allocator's frame

The shared `js_json_parse` entry in both the reference and R1 has a 96-byte
stack frame and saves twelve registers on every call. Empty-object allocation
is inlined into that entry, imposing its register requirements on scalar and
ordinary-object dispatch as well. R2 changes only that allocator's inline
attribute to retain a separate function. All 285 release JSON tests pass.
The matched release build and generated-code/performance checks are pending;
no R2 speedup or regression-free result is claimed yet.

The next acceptance measurements include both original parse/stringify rows
and the full changing-input suite, with default GC and separate retained-output
RSS. [Large-container memory diagnosis](GC_MEMORY_GROWTH.md) is a separate
investigation; neither scan nor outlining is a fix for delayed old reclamation.
