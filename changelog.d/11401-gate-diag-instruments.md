perf(size): compile the mark/classifier verifiers and `hot_diag`'s probes only when asked for (#10572).

`PERRY_GC_VERIFY_MARK` and `PERRY_GC_VERIFY_CLASSIFIER` join the knobs served
by perry-runtime's `gc-instruments` feature, and `hot_diag`'s six mutator
probes (`PERRY_REGEX_DIAG`, `PERRY_IC_DIAG`, `PERRY_LAYOUT_DIAG`,
`PERRY_ENUM_DIAG`, `PERRY_BUFFER_DIAG`, `PERRY_RECEIVER_REPR_DIAG`) move behind
a new `hot-diag` feature with the same contract: both are in `default` (so
`cargo test` and the full prebuilt archives keep everything), the
auto-optimize rebuild adds them when one of their knobs — or
`PERRY_GC_INSTRUMENTS=1`, which now links every runtime instrument — is set at
compile time, and a binary built without them aborts at startup, naming the
knob, when one is set at run time. Without the feature each predicate
(`gc_verify_mark_enabled`, `classifier_verify_enabled`, `hot_diag::*_on`) is a
constant `false`, so everything behind it drops out of the link, and the
relaxed load `enum_on()` cost on every string concat goes with it. The
compiler/runtime knob lists are pinned against each other by
`hot_diag_knobs_match_the_runtime` (new) and
`gc_instrument_knobs_match_the_runtime`, and `hotdiag=` joins the
auto-optimize cache key.

Measured on the issue's shape of program (a 12-line `rich.ts`: a class,
`Array`, `Map`, one regex, `JSON.stringify`, four `console.log`s), Linux
x86-64, auto-optimized, grouped by address: distinct `FUNC` code
6,378,692 → 6,325,848 B (−52.8 KB, −0.83 %); `hot_diag` 26.1 → 0.5 KiB,
`gc::verify` 109.5 → 82.7 KiB; the stripped binary 8,097,680 → 8,035,856 B.
All three arms of the contract were exercised on real binaries: a knob set at
run time on a binary built without it aborts with the message (exit 134); the
same knob set while compiling links the instrument, and `PERRY_ENUM_DIAG`'s
report matches the pre-change binary's; `PERRY_IC_DIAG=0` stays off.

Deliberately NOT gated, and why:
- `PERRY_GC_VERIFY_EVACUATION` (~45 KiB: the evacuation verifiers +
  `verify_diag`) — `gc-native-roots.yml`, `gc_repsel_matrix.sh`,
  `run_memory_stability_tests.sh` and the next-app package fixture set it at
  run time on binaries compiled without it, so gating it turns those arms
  into aborts. It needs those harnesses to set `PERRY_GC_INSTRUMENTS=1` first.
- `gc::trace` and `gc::telemetry` — the issue's diagnostics table lists them,
  but `trace.rs` is the marking tracer and `telemetry.rs` holds `GcStats`;
  both are production collector code.
- `gc::diag_sites` / `gc::survival_diag` — `PERRY_GC_DIAG` output, which many
  scripts read from ordinarily-compiled binaries.
- The per-module `mod-*` gates the issue suggests (`node_stream`, `fs`, `os`,
  …): each needs a keep-alive trace first, as the issue itself says.
