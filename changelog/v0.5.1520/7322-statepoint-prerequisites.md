### Statepoint prerequisites: file-size gate, GC knob arms, and the reason the gate was never green

Clears the two mechanical prerequisites between #7314's opt-in statepoint roots
and their becoming the default, and prepares the third. No emitted byte changes
on the default path.

**File-size gate.** #7314 pushed two files over the 2,000-line cap; both split
along a seam that already existed. `perry-codegen/src/function.rs` 2036 → **952**
(the statepoint/RS4GC lowering into `function/precise_roots.rs`), and
`linker.rs` 2082 → **1618** (its unit tests into `linker_tests.rs`, the same
`#[path]` device that file already used for `linker_temp_lifecycle_tests.rs`).
Pure move: no renames, no signature changes, three items widened to
`pub(super)`.

Verified by **byte-identical emitted IR**, not by "it compiles": both arms built
with the identical package set, binaries hashed and confirmed different, then
`--trace llvm` over 15 modules × 3 modes (default, `PERRY_STATEPOINTS=1`,
`PERRY_RS4GC=1`) — the statepoint modes included because the moved code runs in
no other mode. 39 of 45 `.ll` files byte-identical; the remaining 6 differ
identically under a **same-binary control** (Perry's tagged-template site id and
`__perry_cap_<hex>` capture suffix are not stable across runs of one binary), so
the refactor's contribution to the diff is empty.

**GC knob kill-policy.** Four of #7314's five knobs appeared in no workflow.
`PERRY_STATEPOINT_REPORT` is **deleted** — it was a second spelling of
`--statepoint-report`, so the environment read is gone and the flag is the only
entry point (the driver still sets the variable to reach the rayon workers,
which is now its only role). The other three get arms in `gc-native-roots.yml`,
and each asserts **its own subject was live**, because `PERRY_GC_FORCE_EVACUATE`
passed for months while inert (#6942/#6946):

- `PERRY_RS4GC` — every function record must carry `backend: rs4gc`. RS4GC bails
  *per function* to the explicit bridge on any unrecognised root-alloca shape,
  so a 9/9 green matrix is compatible with RS4GC having rewritten nothing.
- `PERRY_GC_SAFEPOINT_ONLY=strict` — a codegen differential over the whole probe
  glob (statepoints 568 → 530, skipped calls 726 → 764). A strict run that never
  panics proves enforcement was *armed*, not that the contract *did* anything,
  and individual probes show a zero delta, so the assert is aggregate.
- `PERRY_STACKMAP_WALKER` — from the `PERRY_GC_TRACE=1` stream, `verify`
  requires `fp_walks > 0` and `unwind` requires `fp_walks == 0` with
  `walks > 0`. Every walker produces identical program output, so output alone
  can never say which one ran.

New helpers `scripts/statepoint_report_assert.py` and
`scripts/gc_walker_trace_assert.py` carry those assertions;
`scripts/gc_gate_wiring_check.py` now covers this workflow, so `lint` asserts
its wiring.

**Statepoints are aarch64-only today (#7321).** `gc-native-roots` had never gone
green, and not flakily: on x86-64 Linux the compact-map rewriter refuses the
*first* probe — "this module emits an LLVM stack map that the compact-map
rewriter could not parse … Refusing to emit a binary that would lose roots
silently". That is the fail-closed path working; what it changes is scope, since
#7314's drizzle evidence is aarch64 evidence. The matrix moves to `macos-14`,
and the gap is asserted rather than dropped: `statepoints-refuse-x86` requires
that compile to fail *for the compact-map reason specifically*, and goes red the
day x86-64 starts working.

**A latent defect in the same workflow, fixed.** It set
`RUSTFLAGS="-Cforce-frame-pointers=yes"`, which replaces `.cargo/config.toml`'s
`[build] rustflags` wholesale and so dropped `-C force-unwind-tables=yes`. A/B'd
on one tree: without it `09_try_catch_roots` aborts with "unwind tables are
missing from this runtime build (0 frame(s) visible to the unwinder)" and 4 of 9
probes fail `PERRY_STACKMAP_WALKER=verify` with the unwinder visiting **zero**
frames; with it, 9/9. On any host where the x29 chain walk is unavailable the
unwinder *is* the walker, so that configuration would find no roots while forced
evacuation stayed quiet — it enumerates through the same walker.

`gc-native-roots-complete` is a new fan-in job so branch protection needs one
context rather than one per arm. It is deliberately **not** promoted to required
here: a context that has never been green blocks every open PR the day it
becomes required.
