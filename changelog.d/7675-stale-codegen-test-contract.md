### Fixed

**The stale codegen-test contract cluster: five suites that could no longer fail (#7505, #7504, #7503, #6988).**

Four assertions in `crates/perry-codegen/tests/` had drifted from the contract
they name — three of them into shapes that were true of *every* program, which
is CLAUDE.md hazard 4 (the gate runs, its subject does not). Each is replaced
with a claim about a named value, and each replacement was watched go red
against a planted defect.

* **#7505** — `assert_buffer_store_uses_dynamic_fallback` proved "no native
  buffer GEP" with a MODULE-WIDE `!ir.contains("getelementptr inbounds i8")`.
  The shadow-stack lowering's own inline slot addressing (#7088) emits exactly
  that instruction, so sixteen tests reported a stale proof that was never
  emitted the moment anyone ran the suite under `PERRY_RS4GC=0`; #7493 had
  pinned them to native roots to stop the false alarm without making the
  assertion right. `native_proof_support::native_buffer_element_geps` now
  follows the data flow: an `inbounds` GEP is a buffer access only if its base
  was loaded out of a Buffer view's DATA slot, and the helper **panics when the
  function lowered no buffer view at all** rather than certifying its absence.
  All sixteen pins are gone; the tests hold under both lowerings.
  *Sabotage:* removing the data-slot filter — degrading the reader back into the
  module-wide grep — turns 19 tests red under `PERRY_RS4GC=0`.

* **#7504** — `scalar_replaced_slot_roots` counted `js_shadow_slot_bind`
  module-wide, and since #7487 a pooled temp root emits the identical call.
  Every fixture ends in `console.log(o.a, o.b)`, whose argument accumulator
  contributes three binds, so `== 0` was a claim about the accumulator and
  `== 1` a coincidence. New `perry_codegen::testing::root_slots` keys every bind
  and every root-shading barrier by the entry alloca it names, classifies that
  alloca, and **panics on one it cannot classify** so a future slot family goes
  red naming itself instead of being folded into a total.
  *Sabotage:* reverting #6968's `root_entry_alloca` call fails 7 positives;
  disabling #6997's numeric gate fails all three `numeric_only_*` negatives —
  the direction that was silently broken.
  `flat_const_row_aliases_do_not_reserve_shadow_slots` is re-pointed and
  renamed: its two causes separate cleanly and **the temp pool is not one of
  them** (0 binds, 0 reservations there), while the property it asserted would
  be a #6968 bug if codegen satisfied it — the fixture's rows are heap arrays.
  Its second assertion had also gone toothless on its own, forbidding
  `js_shadow_slot_set(i32 1` after #7013 moved the traffic to
  `js_shadow_slot_bind`.

* **#7503** — #7487 re-lowered temp roots onto pooled frame allocas, so
  `js_gc_temp_root_push` / `_get` / `_set` / `_truncate` survive only on an FFI
  fallback arm that neither shipped lowering takes. Ten assertions failed and
  eight `!ir.contains(…push)` negatives held for every program in the language:
  the #6951 / #6969 / #6970 / #6971 / #7114 / #7154 / #7200 contract had no
  working coverage in either direction. New
  `perry_codegen::testing::temp_slots` reads slot traffic in **both** spellings
  (the RS4GC retype preserves register names, which is what lets one reader
  serve both) and states the contract as a claim about a value:
  `assert_rooted_across` proves this producer's result reached a slot and this
  consuming call read its operand back out of it — strictly stronger than
  proving a call existed somewhere in the module.
  `temp_root_operand_temporaries.rs` is 19/19 under both lowerings, unpinned.

* **#6988** — `tests/temp_root_argument_temporaries.rs` is deleted; its seven
  tests moved into `crates/perry-codegen/src/temp_root_coverage/`, an in-crate
  `#[cfg(test)]` module following #7653's `native_root_coverage` pattern, so the
  temp-root emission contract now runs in the required per-PR `cargo-test` job
  instead of the nightly-only integration tier (#5960). Each test runs once per
  lowering.

### Changed

**`scripts/ci_e2e_scope.py` gains one narrow source → suite mapping (#7507).**
A `crates/perry-codegen/src/**` change now selects the three in-process
root-lowering suites that are green — under half a second of test time — closing
the gap that let #7370 take `shadow_slot_hygiene` to 0/12 with nothing red. The
general refusal to map `src/` → suites stands; this is one named exception with
a stated cost, cross-checked against `tests/` on disk so an entry that matches
nothing FAILS the scope step. `native_proof_regressions` and
`native_proof_buffer_views` are deliberately absent while they carry
pre-existing failures: a gate that is red on arrival and cannot block anything
teaches reviewers to ignore it. #7507 stays open for their addition.
