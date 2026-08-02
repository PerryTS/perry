### Fixed

- **A method call's receiver and a computed read's base are now rooted across
  the expressions lowered between them and the dispatch.** Two more sites of
  #7192's root-store-dominance class, both found by extending its checker and
  both reproduced in ~30 lines of TypeScript.

  `recv.m(f())` and `o[f()]` evaluate the reference first and the second
  operand after — spec order, and codegen follows it. That left the reference
  in a bare SSA register while `f()` was lowered, and `f()` allocates. Under
  `PERRY_GC_MOVING_LOOP_POLLS=1` a loop back-edge poll inside it runs an
  evacuating minor. The reference *survives* that minor — the closure capture
  cell, shadow slot or module global holding it is a root — so it **moves**:
  the collector rewrites that location and the register keeps naming
  from-space. This is the same "property (2) — a rewritten location — is
  worthless without property (3), reading that location again below the
  collection point" that `expr/temp_root.rs`'s module header describes, and the
  same fix #7192 applied to the property/element STORE receiver.

  - **`lower_call/console_promise.rs`** — the `js_native_call_method_by_id`
    dispatch. The stale receiver makes the method lookup resolve against
    abandoned memory, so the call throws `TypeError: value is not a function`.
    In `sfw-registry` this is zod's `classic/schemas.ts:301`,
    `inst.regex = (...args) => inst.check(checks.regex(...args))`: `inst` is
    read out of the arrow's capture cell, held across `checks.regex(...args)`
    (a real user call, so it polls), then used as `.check`'s receiver. The
    arguments are now rooted too — each before the *next* one is lowered, per
    `RootedOperands`' incremental contract — so an earlier argument cannot go
    stale across a later one either.
  - **`expr/index_get.rs`** — the dynamic-string-key arm and the last-resort
    runtime-tag-check arm, i.e. the READ counterpart of #7192's `index_set` /
    `property_set` guard, which only covered the store side. The stale base
    makes the field read walk the keys array of from-space memory: a SIGSEGV
    inside `get_field_by_name_object_tail`, or a silently wrong value. In
    `sfw-registry` this is zod's `core/checks.ts:68`,
    `numericOriginMap[typeof def.value]` — a module-global base with a key
    expression that reads a property and therefore can collect.

  Both use `temp_root::guard_store_operand` / `reread_store_operand` /
  `release_store_operand` (#7198's generalized naming). A temp root, not a
  re-lower: re-lowering the reference would observe an assignment made by the
  second operand itself, which is a miscompile rather than a rooting fix. The
  guard emits nothing when the sibling expression cannot collect, so an inert
  argument list or key keeps its previous IR exactly, and it is released
  *after* the dispatch because the dispatcher allocates while reading these
  values.

  Verified by two new gap tests, each red on the parent commit and green after,
  and each clean under a non-moving collector so the failure is proven to track
  collector mode rather than luck:

  | | parent | this change |
  |---|---|---|
  | `test_gap_gc_method_receiver_rooting.ts`, `POLLS=1` | `TypeError: value is not a function` 5/5 | `bad 0` **10/10** |
  | `test_gap_gc_index_get_receiver_rooting.ts`, `POLLS=1` | `TypeError: Cannot read properties of undefined` 4/4 | `bad 0` **10/10** |
  | both, `POLLS=1` + `PERRY_GEN_GC=0` | `bad 0` | `bad 0` |
  | both, default (no polls) | `bad 0` | `bad 0` 5/5 |

### Changed

- **`scripts/gc_root_dominance_check.py` grows a `--stale-registers` mode.**
  The shipped check anchors on a shadow-slot bind, so it can only see values
  that are eventually rooted; neither site above is. The new mode checks the
  more general invariant the module header already states — *no register
  holding a GC pointer may be used below a collection point without being
  re-read from a root* — by classifying every **heap-value source** (an
  allocation, or a read of a collector-rewritten location: a shadow-slot load,
  a closure capture cell, a temp-root slot, a module global, a mutable-capture
  box), following it forward through bit-level identity ops, and reporting the
  first real use that sits below a collecting call. `--fatal-sinks` narrows to
  uses that *dereference* the value (a call receiver or callee), where a
  relocation is fatal rather than merely wrong.

  It reproduces both sites above independently of any runtime probe, and it is
  how they were found. Over the 141-module `sfw-registry` corpus the
  fatal-sink slice went **986 → 729** with this change; the entire
  `js_typed_feedback_native_call_method_by_id` class (257) is gone. The
  remaining 593 are `js_closure_callN` — the generic dynamic-value-call
  lowering, which holds the callee AND the `this` receiver AND each argument in
  registers across the argument list. That is the next site of this class and
  it is not fixed here.

  Like the bind-anchored check the mode is one-sided: `NONCOLLECTING` is the
  only place a call is declared safe. It is a diagnostic, not a gate — the raw
  (non-`--fatal-sinks`) count is dominated by values the checker cannot prove
  are pointers, so it is a ranked lead list rather than a pass/fail number.

  **The exit status says so.** `--stale-registers` prints its counts and exits
  `0`; it is not calibrated to zero, and a mode that returned `1` on any hit
  would be a check that can never pass — the mirror image of CLAUDE.md's four
  "a gate that cannot fail" hazards, and just as reliably ignored. Gating is
  opt-in through the new **`--max-stale N`**, which exits `1` when more than
  `N` uses are reported, so a slice that *has* been calibrated (say the
  `--fatal-sinks` count once `js_closure_call*` is fixed) can become a ratchet
  without the raw mode pretending to be one. Passing `--max-stale` or
  `--fatal-sinks` without `--stale-registers` is a usage error (exit 2) rather
  than a silently ignored budget or an ignored filter — either one would run
  the bind-anchored check while looking like it did something else.
  Misconfiguration keeps its own status: `--min-files` still makes an
  empty corpus exit 2 in this mode too, and the bind-anchored gate that
  `gc-root-dominance.yml` actually runs is untouched — it still exits non-zero
  on any violation.

  `--self-test` grew four arms for this, so the exit status is asserted from
  both ends rather than assumed: over the planted fixture the default mode
  must report 2 uses and still exit `0`, `--max-stale 0` must exit `1`,
  `--max-stale 2` must exit `0`, and the control fixture must report zero.
  Reverting the default to `return 1 if total else 0` fails three of them.
