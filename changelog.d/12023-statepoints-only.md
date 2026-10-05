LLVM statepoints are mandatory on targets with native stack-map support.
Long-lived managed locals retain native homes; the LLVM stack-map address and
allocated-type range describe those homes without per-call relocation fanout.
Compact-map v7 encodes contiguous ranges in constant space.

Remove source-size and 32M relocation shadow fallbacks, per-function retries,
rooting environment overrides and O0 machine-emission escapes. The post-RS4GC
instruction budget fails closed on statepoints. Native runtime argument cells
use the existing handle scanner; shadow frame operations and scanning build
only for targets without native stack maps (and unit-test coverage).

Add executable machine/map size gates, relocation-fanout sabotage coverage,
range corruption checks and a moving-GC test that deleted selectors cannot
disable native rooting. Measurements and validation are recorded in
docs/audits/statepoints-only-12023.md.

The RSS follow-up adds a reproducible Linux executable A/B harness with equal file-cache warming, immutable binary identities, interleaved THP modes, Node output checks and GC receipts. The audit attributes the commander gap to file-backed residency, records 15-pair comparisons and a successful complete Claude Code bundle compile. CPU acceptance remains unresolved on the shared host; no linker or rooting change was promoted.

The cycles follow-up rebases onto pinned main, migrates its new native constructor argument cells to existing runtime handles, corrects profiler-dominated witness RSS, and adds guarded/resumable cycle measurement. Fresh q-size and GC checks pass; strict performance acceptance remains open for quiet-host cycles, qs executable residency and hello startup instructions. See docs/audits/statepoints-only-12023-cycles.md.

The quiet-arm64 follow-up records the coordinator's real qs CPU regression, a guarded Mac build stop, deterministic hello loader-relocation overhead, and a rejected runtime-root storage prototype. The restored implementation passes fresh q-size, GC and runtime checks; performance acceptance and the next rebase remain open. See docs/audits/statepoints-only-12023-arm64.md.
