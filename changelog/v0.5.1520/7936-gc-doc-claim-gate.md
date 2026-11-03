### docs/test(gc): the current collector page's claims are now re-derived, not asserted (#7877)

`docs/src/internals/garbage-collector.md` was created as a current source of
truth and went stale inside the same landing train — five of its architectural
claims were false or misattributed within days, and `memory-model.md`'s entire
source map pointed at a `crates/perry-runtime/src/gc.rs` that the module split
had deleted, line numbers and all. Correcting the prose again was not the fix.

**`scripts/check_gc_doc_claims.py`** (in `lint`, a required context; also on the
Windows structural-audit arm) holds the checkable half of those pages to the
code:

* **Paths.** Every repo path a current GC page cites must exist, and a
  `path:LINE` citation is rejected outright — a line number is a claim nothing
  re-derives. This rule found two more dead paths nobody had reported:
  `CLAUDE.md`'s add-a-widget recipe still routed contributors to
  `crates/perry-codegen/src/codegen.rs` and `crates/perry-hir/src/lower.rs`.
* **Numbers.** A documented constant carries
  `<!-- gc-fact: NAME = VALUE in PATH -->` and is compared against the `const`
  in that file; a source-map row carries `<!-- gc-symbol: NAME in PATH -->` and
  must still name a definition. 17 facts are re-derived. The rule asserts its
  own subject was live — deleting markers instead of fixing them trips a floor,
  because a rule with an empty population passes vacuously.
* **Behavioural claims cite their test.** The four claims that most recently went
  false — weak-holder slicing, the process-wide pool cap, the sticky
  critical-pressure drain, in-place promotion — carry a `gc-symbol` marker naming
  the test that proves them. Deleting or renaming that test fails `lint`;
  changing the behaviour fails `cargo-test`. This is the "cheap source-marker
  test for collector changes" the reopened issue asked for, in a form that can
  actually fail rather than a checklist item.
* **Tracker attribution.** The operations page names no issue numbers at all.
  "That phase is atomic and unsliced (#7874)" became false the moment #7874
  closed and nothing in the tree noticed; an issue's state is not a fact this
  repository holds. Off that page the rule is narrow ("#N tracks", "tracked by
  #N", "blocked on #N"), so historical causal references survive.

The script documents what it cannot catch — prose that is simply wrong about
behaviour — which is the reason rule 2 exists at all.

#### What was corrected, each verified against code

* **Weak processing** is registry-scoped on every path: full/fallback collection
  snapshots the holder registry and consumes a bounded number of holders per
  step (`FullWeakProcessingState`), so it is O(registered weak holders) and
  resumable, not an unsliced arena walk.
* **The block reuse pool**'s LIFO ordering is per-thread but its cap is
  **process-wide** and budget-scaled (one eighth of a device/container budget,
  1 MiB floor), and a critical-pressure drain request is sticky — it empties the
  pool once the owed full collection finishes its arena reclamation.
* **Old-page defragmentation** has its mutable-root rewrite contract; it is
  opt-in because no fragmentation workload in the corpus exercises it, not
  because the contract is missing.
* **Tenuring** is an adaptive 1–4 threshold with a survival-rate lock, not a
  fixed `PROMOTION_AGE = 2`; the flat `HAS_SURVIVED`/`TENURED` pair describes
  the non-copying path. Whole-block in-place promotion, its untraced arm, its
  two footprint budgets, and the type-dependent born-old thresholds (16 KiB
  pointer-free, 128 KiB pointer-bearing) are now documented, all bound to their
  constants.
* **`js_arena_stats`** reports an exact post-collection live census, so the
  ratchet README's advice that a whole-block jump in `heap_used_bytes` means
  "one object too many, not 1 MiB too much" had inverted; ~1 MiB now means
  ~1 MiB of objects.
* **`CLAUDE.md`** pointed at `gc/mod.rs` for a scanner registry that lives in
  `gc/roots.rs` and claimed "~55 entries" against a population of 123. The count
  appeared twice and only one copy was corrected by hand — so the number is gone
  from the prose and `scripts/gc_runtime_root_holders.py`, which computes and
  floors it, is cited instead.

Two inline literals became named constants so the page could cite them:
`BLOCK_POOL_CAP_DEFAULT_BYTES` and `SCAVENGE_NURSERY_CAP_DEFAULT_MB`. Neither
changes a value or a code path.

#### Validation

Sabotage-verified with the fix committed first: falsifying a documented number
and changing the constant in the source each fail (exit 1, checked unpiped);
renaming `tenuring_survivals` fails the symbol rule; adding an issue reference to
the operations page fails; deleting every marker fails the vacuity floor. The
`--self-test` arm asserts the same four shapes synthetically.
`gc_gate_wiring_check.py`, `check_file_size.sh` and `cargo fmt --check` clean;
the 19-program GC corpus is byte-identical to `node --experimental-strip-types`
with exit 0 on the rebuilt runtime.
