Guarded record-stringify release: qualified performance and correctness evidence.

Measured source commit: df32314ace602869c8a75584981a0e7bd0d0f7f0. All 39
source hashes in provenance.json were checked against that commit. The worker
and archive hashes identify the release independently of later source edits.
The baseline arm is the Unicode checkpoint, not stock v0.5.1520.

The full run passed the load/competing-process gate and all 35 external monitor
observations were clean: 152 output verifications, 456 timing trials, 432 memory
trials. The separate paired check contains 252 trials across 18 cases, with
20 clean external observations and its own passing gate.

Inventory: 11/38 CPU, 58/74 peak RSS, 28/36 retained RSS medians at or below
the better Node/Bun median. This does not meet the all-row goal. Paired checks
confirm remaining regressions, including numeric parsing versus Unicode,
small-object parsing versus tape, and empty-object stringify after the required
prototype-lookup root fix. See recheck-data-record/summary.json for all ranges.

The local-instructions and local-profile subdirectories are explicitly local
diagnostics. Their process-wide counters and samples are not quiet-host CPU
or RSS acceptance measurements.
