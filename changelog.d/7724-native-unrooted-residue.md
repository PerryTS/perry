### gc-root-dominance-statepoints: the phi false positives are gone, and the real residual needed one more hit than the last snapshot said

`--max-unrooted` goes **8 → 2**. #7664 asked for two things: fix the
checker's four phi-mediated `unmasked` false positives, and fix whatever real
hits remained underneath them. Re-verifying found a fifth real hit the prior
triage comment didn't have — `test_gap_static_method_value_name_collision`,
whose receiver-arming code landed with #7691 after that triage ran — so the
honest floor going into this PR was **9 hits total**, not the 8 the budget
still said: 4 phi false positives, 3 `unrooted:global`, 2 `unrooted:capture`.

#### The checker: a phi operand's window is its own edge, not the join

`scripts/gc_root_dominance_check.py`'s `chain` (the untracked cast-closure a
stale use is searched in) treated `phi` as unconditionally transparent: one
tainted incoming edge blanket-tainted the phi's *result*, and any downstream
use of that result was then checked for a safepoint on *any* CFG path between
the source and the use — `between_blocks` is deliberately path-insensitive,
which is sound for ordinary registers but not for a phi, where the dynamic
value at the join depends on which edge was actually taken. All four false
positives were the same `&&`/`||` short-circuit join: the tainted operand's
own edge never crosses a safepoint; the *other* edge does, and the checker
was reporting that.

Fix: `_cast_closure` gained `phi_all_edges` — a phi only joins `chain` once
*every* incoming edge is independently in it (the worklist retries a
partially-satisfied phi from each operand's own arrival, so admission order
doesn't matter). That closes the false positive, and deliberately gives up
catching a hazard on a *single* tainted edge with its own intervening
safepoint — `_phi_edge_hazard` covers that case separately, checking each
edge's window against its own predecessor's terminator instead of the join.
Two new self-test fixtures pin both directions: `phi_safe_edge` (the false
positive shape, must report 0) and `phi_hazard_edge` — byte-identical except
the safepoint moves onto the tainted edge — which must report exactly 1. Each
was verified against a sabotaged copy of the checker to confirm it can still
fail (disabling `_phi_edge_hazard` fails `phi_hazard_edge`; reverting
`phi_all_edges` fails `phi_safe_edge`).

#### Fixed: 3 `unrooted:global`

`(Lexer as any).lex(...)` (marked's `Lexer.lex`/`Parser.parse` static-value
collision shape, `lower_call/property_get/static_dispatch.rs`) computed its
receiver once, then held it raw across arg-bundling logic that always
allocates when the resolved method has a synthesized rest param, and can
allocate for arbitrary argument expressions otherwise —
`implicit_this_save`/`js_static_this_arm_value` then read the stale copy.
Wrapped the receiver in `RootedGroup::adopt`/`reread` (the "root a value the
caller already computed" combinator), with `collects` derived from the same
predicate `operand_protection` uses elsewhere rather than hardcoded true — a
plain zero/literal-arg static call still emits no rooting traffic.

The other two (`new DataView`/`new Uint8Array` reading a module-global
receiver, then holding it across a sibling `{ valueOf() {...} }` argument's
allocation, in `test_gap_arraybuffer_transfer`) turned out to already be
fixed on `main` by the time this branch rebased: #7719 landed the identical
rooting shape across all 30 of `lower_call/builtin.rs`'s constructor arms via
a shared `RootedGroup`, a superset of what this PR would otherwise have
needed to add there.

#### Open: 2 `unrooted:capture` — `js_closure_get_capture_bits`'s result is never re-entered into either protected domain

Diagnosed further than the prior triage's "signature/ABI change" note, and
it's narrower than that reads: `js_closure_get_capture_bits` returns a raw
`i64` that may be a NaN-boxed heap value. When the caller re-enters it into
the RS4GC-tracked `ptr addrspace(1)` domain (the closure-pointer masking
idiom `codegen/closure.rs` already uses for `%this_closure` itself) or into a
temp root, it's fine. The generic "read a captured value for arithmetic /
forwarding" call sites (`expr/mod.rs`, `literals_vars.rs`, `array_push.rs`,
and the `js_new_function_construct` callee case in the synthesized
dynamic-parent implicit constructor) do neither — the value is used as a bare
`double`/`i64` and stays that way across whatever the rest of the function
does. `test_gap_class_expr_dynamic_parent_ctor::__closure_21` and
`test_gap_computed_key_method_nested_this::__closure_9` are both this shape,
confirmed register-by-register against the corpus IR.

The natural fix mirrors the string-handle precedent already in
`root_reload.rs`: treat a `js_closure_get_capture_bits` call as a
*reloadable* source (re-emit the same call rather than a load — the same
closure's same capture index, provably unmodified as long as no
`js_closure_set_capture_bits` to that index intervenes, the same store
side-condition the existing pass already tracks for shadow slots and handle
globals). That's a real engineering slice — `root_reload.rs`'s `Facts`
currently only models *loads* as reloadable sources, not calls — not a quick
follow-up, so it stays open. `--max-unrooted 2` is these two hits exactly.

`gc-root-dominance` (shadow-stack) is unaffected — none of this touches the
shadow lowering's own corpus or `root_reload.rs`'s existing reload rule.
