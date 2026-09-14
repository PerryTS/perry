# Native-side property Get: measured ordinary-data path

Base: `9fda98df68d9fac3c08b2385fae007aa9f5278df` (main, 0.5.1563).
Date: 2026-09-14. Measurements: Linux x86_64, perrymaster, AMD Ryzen 7 7700X.

## Change and safety boundary

`Reflect.get`, the named-field ABI, and the native borrowed-name getter share
an inlinable positive lookup. It classifies the pointer once per chain link,
requires an arena-backed GC object with an ordinary shape and an admitted
ordinary class ID, checks the accessor-key Bloom summary, then reads the
current shape's own data slot. It follows explicit ordinary prototypes or
already-rooted synthetic class prototype pointers for at most 32 links.
It does not infer absence: every unproven case falls through to the original
Get implementation. Keys remain borrowed throughout this callback-free,
JS-allocation-free lookup; the consult-only keys index cannot build or collect.

No new GC roots or pointer caches are introduced. The `toJSON` caller retains
its handle scope, reuses the rooted canonical key, and uses the new canonical
key Get API. Default builtin prototype resolution remains on the slow path:
it can initialize objects and observe replacement through globalThis.

Own/prototype accessors, proxies, private-looking names, `constructor`, declared
classes, process.env, mapped arguments, element-bearing objects, and native
exotics decline. In particular, RegExp expandos and `lastIndex` still use the
existing exotic handler. Inherited null/undefined values conservatively decline
to preserve the current slow resolver's behavior. A distinct Reflect receiver
can use a data hit because no getter or receiver binding is observed.

This deliberately limits the optimization to ordinary data reads. The measured
improvement does not require exotic bypasses, new per-realm builtin pointers,
or a per-call-site cache. Remaining hot costs include class-ID membership and
prototype registries; those have separate invalidation and collision rules.

## Instruction evidence

The base workloads were measured before runtime edits. All five programs check
their checksum and match pinned Node 26.5.1 stdout. Both builds use the same
checkout base, package set, release codegen-units=16, debug=1, strip=none, and
repository unwind/frame-pointer flags. Probe compilation uses
`--no-auto-optimize --debug-symbols` and an explicit `PERRY_RUNTIME_DIR`.
The table uses medians of three `perf stat -e instructions:u` runs, including
startup. It makes no shared-host wall-clock speed claim.

| Workload | Iterations | Base instructions | Candidate instructions | Reduction | Inclusive Get share, base → candidate |
|---|---:|---:|---:|---:|---:|
| `reflect_own` | 200,000 | 498,582,567 | 143,385,957 | 71.24% | 98.80% → 97.20% |
| `reflect_proto` | 200,000 | 1,369,594,468 | 305,995,648 | 77.66% | 98.90% → 99.35% |
| `iterator` | 200,000 | 886,642,572 | 886,162,527 | 0.05% | 0.68% → 0.79% |
| `thenable` | 20,000 | 365,634,266 | 320,441,221 | 12.36% | 15.50% → 2.66% |
| `tojson` | 100,000 | 742,646,720 | 423,858,278 | 42.93% | 50.71% → 28.22% |

`iterator` is a user `for…of` iterator that reuses its result object: generic
Get is under 1% of the baseline, so the neutral result is expected. Positive
thenable assimilation and inherited `toJSON` expose the runtime cost clearly.
The synthetic Reflect probes cover own data and two prototype links.

Attribution uses `perf record -e instructions:u -c 500003 --call-graph
dwarf,16384 -m 64M --no-buildid --no-buildid-cache`, then `perf script --no-inline`.
All ten final profiles reported no lost samples. Folded stacks are weighted by
sample period, and nested Get frames count once in the inclusive share. Shares
are sampling estimates, especially the small iterator share. An earlier smaller
sampling buffer dropped samples; those attribution runs are excluded here.

The JSON files in `evidence/` record all counts, shares, top inclusive-Get leaf
symbols, probe/binary hashes, and compiler/runtime archive hashes. The compressed
`measurement-receipts.tar.gz` contains raw stat rows, checked stdout, final
folded stacks, perf record logs, and the measured candidate source patch.
The patch records the pre-commit source snapshot; later evidence and harness
reporting edits do not change the measured runtime. Full perf data and linked
probe binaries remain under `/root/native-property-get/` on perrymaster.

## Correctness and fault injection

Eight Rust tests compare ordinary reads against a forced slow path and cover
all value kinds, prototype mutation, shadow/delete/freeze, own and inherited
getters, setter-only descriptors, proxies, class statics, private-looking keys,
arrays, Buffer, mapped arguments, RegExp expandos/accessors, long keys, and
spilled/deleted shape slots. Getter counts assert one invocation per Get.
`test-files/test_gap_native_property_get.ts` adds 42 lines of checked parity output,
including distinct receivers, proxy traps/revocation, key-conversion order,
symbols, class/private syntax, Date, typed arrays, and RegExp.

The test-only forced-slow switch is thread-local; it cannot affect production.
`faults.py` runs healthy tests before and after each serial source-mutation
campaign, restores source files in `finally`, and requires a named assertion
failure (a compile failure does not count). All three mutations were killed by named assertion failures (exit 101):

| Mutation | Failing test |
|---|---|
| Force every classified Get to decline | `own_data_is_served_and_matches_forced_slow_for_all_value_kinds` |
| Ignore the accessor Bloom guard | `accessor_bloom_guard_preserves_own_and_inherited_getter_calls` |
| Ignore RegExp own `value` expandos in the fallback | `regexp_expandos_and_accessors_remain_on_the_exotic_path` |

The last mutation exercises the preserved exotic fallback; this change adds no
exotic fast path. The restored eight-test suite passed before and after the
campaign. See `evidence/faults.json` and `fault-logs.tar.gz`.

## Validation

- Full runtime unit suite: 3,803 passed, 4 ignored, 0 failed, single-threaded.
  Base suite: 3,795 passed, 4 ignored; the known native-stack test passed here.
- `cargo check -p perry-runtime --no-default-features --features full`: passed.
- Clippy all-target diagnostics compared with the captured base: no additions
  or removals after normalizing shifted line numbers. Both runs retain the same
  12 pre-existing `approx_constant` errors; see `evidence/clippy-diff.json`.
- `cargo fmt --all --check`, `git diff --check`, and Python syntax checks passed.
- The final measurement harness smoke test passed all five programs on Linux.
  The 42-line parity program matches Node on both base and candidate.
- The final full lint replay ran both script and compile tiers: 80 of 83
  commands passed, with three existing failures and two CI-only skips. A new test-only
  boolean initially required a custody verdict; the explicit `test_only` entry
  fixes that failure, with no added pointer debt. The script-tier replay passes
  every executable gate except the pre-existing public benchmark freshness red.
  The compile tier also exposes pre-existing unused WebAssembly helpers, an existing test cache assignment, and API
  docs drift (`bun-pty`). The unused-helper failures reproduce on a clean base
  worktree. All ten API-manifest source files are byte-identical to base, and
  the generated diff is saved. See `evidence/base-gate-comparison.json`.
- GC ratchet: all 14 probes match Node, with seven measured repetitions and two
  trace passes on both base and candidate. All 126 shared-CI metric medians
  (including the probe-specific exclusions) are exactly identical. The checker
  passes against a fresh same-host base artifact. The repository's older pinned
  artifact fails on both base and candidate; it was not changed or re-pinned.
  See `evidence/gc-ratchet-ab.json` and the raw GC receipts.
- GC stress: all 86 corpus programs ran across seven PR arms: 428 PASS, 167
  UNVER (output matched but that arm did not collect), 7 FAIL. Every required
  arm passes aggregate liveness. All seven failures are the HTTP/2 pending-event
  callback fixture. A fresh-base replay reproduces all seven failures with
  byte-identical evidence strings and collector counters. See
  `evidence/gc-stress-base-http2.json` and `gc-stress-receipts.tar.gz`. The
  single-fixture replay has inert arms as expected; the full candidate corpus
  satisfies every arm's liveness requirement.

Main advanced to `eb13fa188d` while this work was validated, including separate
GC and HTTP fixes. The measurements and A/B receipts deliberately remain on
`9fda98df68`; the merge train must replay the combined tree. This branch does
not absorb or rebaseline those independent changes.
