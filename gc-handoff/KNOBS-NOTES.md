# KNOBS-NOTES — #7991 / #7982 / #7737

Working notes for the "small, well-specified, independently shippable" batch.
Written incrementally; the durable conclusions are the inventory table and the
two stale-fixture findings.

---

## #7991 — `PERRY_GC_DIAG=0` ENABLED diagnostics — **DONE**, PR #7993

### What shipped

`gc/telemetry.rs`'s `var_os("PERRY_GC_DIAG").is_some()` is presence-only, so
every value turned diagnostics on. Routed through the #5093 parser, and the
audit turned that one-liner into a family fix.

Two vocabularies now exist, both **pure functions of the raw value**
(`gc/mod.rs`):

* `env_flag_from_value` — default-OFF. True for `1`/`true`/`on`/`yes`
  (case-folded, trimmed). False for unset, `0`/`off`/`false`/`no`, empty, and
  **anything unrecognised**.
* `env_default_on_from_value` — default-ON kill switch. False only for an
  explicit `0`/`off`/`false`/`no`. Unrecognised ⇒ ON.

They are **not** each other's negation, on purpose: each fails toward its own
documented default. That has its own assertion so a later tidy-up cannot
collapse one into the other.

### The knob-parse audit inventory (GC family, production code)

Scope: `crates/perry-{runtime,codegen}/src`, `crates/perry/src`, excluding
tests. "Was" = state on `origin/main@55fd197d5`.

| knob | was | sites | now |
|---|---|---|---|
| `PERRY_GC_DIAG` | **presence-only** | 20 (18 `is_some`, 2 `is_none`) | `env_flag_enabled`, cached |
| `PERRY_GC_VERIFY_MARK` | **presence-only** | 3 | `gc_verify_mark_enabled()`, cached |
| `PERRY_GC_VERIFY_RS_NONFATAL` | **presence-only** | 1 | `env_flag_enabled`, cached |
| `PERRY_GC_VERIFY_EVACUATION` | **split-brain** | 2 | both value-parsed |
| `PERRY_SHAPE_LAYOUT_KEYED` | `v != "0"` | 1 | `env_default_on_enabled` |
| `PERRY_GC_TRACE` | `1\|on\|true` | 1 | `env_flag_enabled` |
| `PERRY_GC_VERIFY_CLASSIFIER` | `1\|on\|true` | 1 | `env_flag_enabled` |
| `PERRY_GC_FORCE_EVACUATE` | `1\|on\|true` | 1 | `env_flag_enabled` |
| `PERRY_GC_SAFEPOINT_ONLY` | `1\|on\|true` + `strict` | 1 | `safepoint_only_contract_from_value` |
| `PERRY_GEN_GC` | `!(0\|off\|false)` | 1 | `env_default_on_enabled` |
| `PERRY_WRITE_BARRIERS` | `!(0\|off\|false)` | 1 | `env_default_on_enabled` |
| `PERRY_GC_MOVING_SAFEPOINT` | `!(0\|off\|false)` | 1 | `env_default_on_enabled` |
| `PERRY_GC_INCREMENTAL` | `!(0\|off\|false)` | 1 | `env_default_on_enabled` |
| `PERRY_GC_PROTECT_FROMSPACE` | `parse_protection_mode` | 1 | unchanged (already correct) |
| `PERRY_GC_PROTECT_FROMSPACE_DEPTH` | `parse_quarantine_depth` | 1 | unchanged (numeric) |
| `PERRY_GC_FROMSPACE_SCAN(_ABORT)` | `resolve_scan_knobs` | 2 | unchanged (already correct) |
| `PERRY_GC_PROMOTE_IN_PLACE` | `parse_promote_in_place` | 1 | unchanged |
| `PERRY_GC_OLD_DEFRAG` | `old_page_defrag_enabled_from_value` | 1 | unchanged |
| `PERRY_GC_SCHEDULE_{SEED,RATE,ALLOC_KB}` | dedicated parsers | 3 | unchanged |
| `PERRY_GC_MOVING_LOOP_POLLS` | `moving_loop_polls_enabled_from_env` | 2 | unchanged (runtime + codegen) |
| `PERRY_GC_{HEAP_LIMIT,SCAVENGE_NURSERY_MB,MAJOR_PACING_*}` | numeric | 4 | unchanged |
| `PERRY_CONSERVATIVE_STACK_SCAN` | `..._from_value` | 1 | unchanged |
| `PERRY_STACKMAP_WALKER` | value match | 1 | unchanged |
| `PERRY_RS4GC` / `PERRY_SHADOW_STACK` (codegen) | `1\|on\|true` / value | 2 | unchanged, different crate |

**Non-GC presence-only reads that remain** (out of scope, listed so the next
audit does not have to re-derive them): `PERRY_DEBUG` ×15,
`PERRY_SUPER_DEBUG` ×3, `PERRY_BIGINT_MIX_DIAG` ×2, `PERRY_MT_PROFILE` ×2,
and one each of `PERRY_TYPED_FEEDBACK`, `PERRY_TYPED_FEEDBACK_TRACE`,
`PERRY_TRACE_OBJECT_ARRAY_WRITE_GUARD`, `PERRY_REJECTION_DIAG`,
`PERRY_PLAN_DIAG`, `PERRY_I18N_DEBUG`, `PERRY_EXPERIMENTAL_VM_MODULES`,
`PERRY_TTY_TEST_PIPED_CHILD`. Same bug shape, lower stakes — none of them is
an A/B control. `PERRY_EXPERIMENTAL_VM_MODULES` is the one worth a look: it
is a *capability* switch, so `=0` enabling `vm` module semantics is a
compatibility question rather than a measurement one.

### Teeth

1. `gc/tests/env_knob_parse.rs` — 6 tests. The pure cases pin the vocabulary,
   but they would **all stay green** under a revert of that one line, so the
   decisive case observes the live cached reader in a **child process** under a
   real `PERRY_GC_DIAG=0`/`off`/``/`1`. ON arm included: a fix hard-wiring
   `false` must not pass either.
2. `scripts/check_gc_env_knobs.py` (in `lint`) rejects the presence-only shape
   for the GC family. `PRESENCE_ONLY_ALLOWED` is empty; a **stale** entry also
   fails. `--self-test` sabotages the detector with the exact shipped shape.

Also: `PARSER_RE` in that script had to learn the two helper spellings.
Routing a knob through `env_flag_enabled("NAME")` made the *existing*
"no live parser owns it" check fire on 7 knobs — worth knowing if anyone else
factors an env read behind a helper.

### Sabotage verification

Fix committed first (`871aa3e44`), then `telemetry.rs` reverted **in place** to
`var_os(..).is_some()`:

* test: `FAILED … PERRY_GC_DIAG=Some("0") must read as OFF`
* lint: `PERRY_GC_DIAG: read for presence … 'PERRY_GC_DIAG=0' would ENABLE it.`

Restored **and rebuilt** (not merely `git checkout`-ed) → 6/6 green.

---

## #7982 — `PERRY_LLVM_INPROCESS=native` vs RS4GC IR — **DONE**, PR #7998

### The reader family (the issue's warning was right)

Six shapes, each exposed by fixing the previous one:

| shape | fix |
|---|---|
| `alloca ptr addrspace(1)` | `basic_type` addrspace arm (the reported failure) |
| `store ptr addrspace(1) null, ptr %r` | `ty_and_val` — the qualifier is part of the TYPE but sits after a space |
| `null` in an addrspace operand | `constant` must null in the operand's address space |
| `define … "frame-pointer"="non-leaf" gc "statepoint-example"` | string-attribute arm; `gc "…"` lifted out like `personality` (it contains a space) |
| `call … ) "gc-leaf-function"` | string-attribute arm on callsites |
| `landingpad token cleanup` | built via llvm-sys; `token` is not an inkwell `BasicType` |

### Two defects hiding BEHIND the reader failure — both worse than it

1. **The native path returned assembly and called it an object.** Statepoint
   backends compact the stack map at assembly time, so the plan carries `-S`.
   The textual path has always run rewrite-and-assemble; native and diff
   returned raw bytes → `ld: unknown file type`. `linker.rs` literally carries
   a comment predicting this, on the path that already handled it. Now
   `linker::finish_native_emission`, wired into all four emit sites.
2. **A natively-constructed module had NO GC strategy, so RS4GC never ran.**
   `synth_define_header` was a second copy of `to_ir`'s header renderer, written
   before `"frame-pointer"` and `gc "statepoint-example"` existed. Verifies,
   links, runs correctly on any non-collecting program — with **no precise
   roots at all**. Invisible to a behaviour-parity smoke arm *by construction*.
   Fixed structurally (one `LlFunction::define_header`), pinned in `function.rs`
   which compiles WITHOUT the feature, so per-PR CI sees it.

   ★ The pin took THREE attempts, and the first two failed the same way the bug
   did. (a) The `to_ir == define_header` agreement test cannot see a dropped
   `gc "statepoint-example"` at all — one renderer means both sides change
   identically; sabotage passed. (b) A dedicated strategy test that BRANCHED on
   `native_stack_roots_enabled()` never ran its ON arm under `cargo test`: no
   module has called `set_native_roots_for_target`, so the predicate is false in
   the test process and the sabotage passed again. Only (c), pinning both
   lowerings with `NativeRootsPin::{native,shadow}` and asserting
   `stack_map_slot_count` in each arm so neither is vacuous, goes red on the
   sabotage. Reproduced this PR's own bug class twice inside its own test.

### Are the `.ll` corpora live or still frozen? — **LIVE**

Refreshed to 497 / 1082 / 420 `addrspace(1)` sites (were 0/0/0). Two mechanisms
so they do not re-freeze:

* `scripts/refresh_llvm_inprocess_corpora.sh` (regenerate; `--check` diffs).
* `scripts/check_llvm_corpus_currency.py` in `lint` — asserts every IR form the
  reader has a branch for is PRESENT. **Sabotage-verified against the
  `origin/main` corpora: names all 10 forms they lacked.**
  *Stated limit*: catches a form that VANISHES, not one codegen newly invents.
  Nothing static can; the end-to-end `native` arm is the closure for that
  direction. Which is primary? The refresh script is the mechanism, the census
  is the alarm, and the census is what makes the unit corpora non-decorative —
  I consider the census primary, the script its prerequisite.

### ★ The existing currency diagnostic was ITSELF vacuous

`llvm-inprocess.yml`'s "Corpus currency" step asks `git log` how many
IR-affecting commits landed since the corpora changed — on a **depth-1**
checkout (no `fetch-depth` anywhere in either workflow). One commit to answer
from ⇒ it printed a confident **`0`** however stale the files were. A
diagnostic reassuring readers about the exact thing it exists to detect. Now
`fetch-depth: 0` + it fails loudly rather than printing zero.

### Not closed (separate defects; neither was EVER reached in CI)

* `=diff` byte mismatch on the spike: 149,105 text vs 163,902 native. It was
  50,995 before the GC-strategy fix, so the gap went from "RS4GC never ran" to a
  real but far smaller divergence. Pre-opt prints differ by design (C-API
  constant folding), so this needs its own investigation.
* Unit-split diff arm: `call to undeclared @js_shadow_slot_set` — the per-unit
  skeleton does not declare it.

Both were dark because the native arm failed first. `llvm-inprocess` has **no
success in its last 60 runs**.

---

## #7737 — GC follow-ups from #7733 — **TRIAGED**, 3 of 4 already landed

Commented on the issue (#7737 comment). Summary:

| item | state |
|---|---|
| 1. one-way `OBJECT_PROTOTYPES_NONEMPTY` latch | **DONE** on main — released on prune drain, latched under the lock, `mod latch_drain_tests_7737` |
| 2. positive-direction pacing test | **DONE** — `escalating_records_the_pre_full_arena_reading`; the "not mockable" blocker closed by a `#[cfg(test)]` seam on `pacing_escalation_reading_bytes()` |
| 3. grow-then-churn probe | **DONE** and baselined — and it **refuted** the issue's prediction: the boundary moves 4× but peak RSS moves 3% the OTHER way, with two fewer fulls for identical collector work |
| 4. `gc-ratchet`/`gc-stress` not required | **OPEN, and newly blocked** |

★ Item 4's stated prerequisite is now stale in the opposite direction. The
issue says "recent runs now satisfy" the observed-green-once requirement.
Measured today: `gc-ratchet` has **0 successes in its last 30 `main` runs** (13
failure, 15 cancelled; latest completed ends `gc-ratchet: FAILED`), and
`gc-stress` failed in each of the last three completed `main` runs of
`test.yml`. Promoting either today would block every open PR — the exact
corollary CLAUDE.md states. Green on `main` first, then promote. Branch
protection is a maintainer action either way.

Second-order: 15 of 30 `gc-ratchet` `main` runs were **cancelled**, i.e. never
executed. That is CLAUDE.md's third way a gate cannot fail, and it makes
"observed green once" harder to reach than the run list suggests.
