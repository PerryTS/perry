# Opening-container scan and parse entry experiments

These are follow-ups to the corrected parser landed through PR #10035. The
current R3 candidate is undergoing final performance validation in PR #10036. Global GC policy, construction
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
The matched release build passed in 5m34s. The generated entry shrank from
1736 to 156 bytes and its frame from 96 to 16 bytes; parse_slow also shrank
from 7232 to 5416 bytes. Compiled Node comparisons and live moving/full-GC
witnesses pass. [Exact validation and codegen evidence](results/opening-scan-r2-validation/README.md).
The [full original matrix](results/quiet-opening-scan-r2-all-r5/README.md) and
[full changing-input suite](results/quiet-opening-scan-r2-rotating-r5/README.md)
are complete. R2 repairs the tiny-input regressions and retains the large-string
gains, but sparse reads (+0.614%), heterogeneous parsing (+0.467%) and rotating
wide-object parsing (+0.473%) show separated slower ranges. PR #10036 remains
draft. The corrected baseline tree has landed on main through #10035.

## R3: isolate the long search tail

R3 keeps the initial 16-byte positive check inline and outlines the wide search
tail from the depth state machine. All 285 release JSON tests, the matched
release build, compiled Node comparisons and live moving/full-GC witnesses pass.
The depth scanner shrinks from R2's 3904 to 3676 bytes; its separate tail is
284 bytes. The lean parse entry and ordinary string-constructor sizes remain
unchanged. [Validation and codegen](results/opening-scan-r3-validation/README.md).

The [complete original suite against freshly built merged main](results/quiet-opening-scan-r3-main-all-r5/README.md)
passes all 200 output checks and retains 46/50 CPU, 67/86 peak-RSS and 31/36
retained-current-RSS wins over the better Node/Bun median. No row has separated
slower timing ranges, but sparse reads (+0.50%), heterogeneous parse (+0.23%)
and variable large-string stringify medians (+5.43%/+3.92%) require focused
replay. Overlap alone is not proof of equality. Full changing-input validation
against that fresh main build is running; PR #10036 remains draft.

Earlier [long original-worker replay](results/quiet-opening-scan-r3-regression-r9/README.md)
and [changing-input focus](results/quiet-opening-scan-r3-focus-r9/README.md) used
the older corrected-R2 reference. The fresh focus retains approximately 7.5%
ASCII, 4.5% Unicode and 3.4% 1 KB object parse gains without separated slower
ranges. These reference binaries are identified separately in every result.

[Large-container memory diagnosis](GC_MEMORY_GROWTH.md) is a separate
investigation; neither scan nor outlining fixes delayed old reclamation.
[Pristine Linux main CI comparison](results/main-e722-ci-comparison/README.md)
reproduces the earlier correction PR's seven gap failures and stack-size test
failure; this does not substitute for CI on the current candidate.
