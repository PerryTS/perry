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
