Install the GC diagnostics (`PERRY_GC_LOG`, `PERRY_GC_DIAG`,
`PERRY_GC_VERIFY_EVACUATION`, and the `PERRY_GC_TRACE` JSON) when one of
their knobs is set while compiling, the same pay-for-use rule the instrument
knobs follow. Since these diagnostics moved behind
`js_runtime_install_diagnostics`, a program whose installs are chosen from its
own features (every WASI link and every auto-optimized native link) installed
them only for a heap-snapshot or `process.report` call. Setting the knobs only
at run time therefore printed nothing and skipped the evacuation verifier
without any warning. The WASI image selftest's moving/full-collection gate was
red on main for this reason. A compile with none of the knobs set produces a
byte-identical binary. The image selftest, the wasip1 reference-host
`discard_reuse` case and the guest moving-GC component check now pass their GC
knobs at compile time too. The selftest checks moving collections with the
verifier's `evacuation_ok` lines: the `copying_minors` counter only counts in
instrumented archives.
