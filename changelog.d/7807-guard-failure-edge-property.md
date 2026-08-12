**`typed_f64_receiver_method_clone_raw_loads_after_composed_guards` is green again, and now asserts the property instead of a symbol name** (#7506). `native_proof_regressions` goes from 261/262 to **262/262** and moves into the per-PR map.

#7506 posed the question directly: the guard-failure edge stopped calling `$generic`, so either the assertion is stale or a receiver whose fields are not raw f64 takes the typed clone's raw loads anyway. Answered by reading the emitted IR rather than by choosing.

The composition still has **three** outcomes, not two:

1. method-direct guard fails → `js_native_call_method_by_id` (fully dynamic)
2. raw-f64 field guard passes → `$typed_f64_recv`, which raw-loads the receiver's slots with no coercion
3. raw-f64 field guard **fails** → `$pshape`, a Ptr<Shape> clone

So (3) changed callee, not existence. And `$pshape` is sound for that edge for a reason the old assertion could not express: it *does* emit `inttoptr` + `getelementptr` + `load double` — the shape guarantees the slot OFFSETS — but routes every loaded slot through `js_number_coerce`, which is exactly the right handling for a slot that may hold a NaN-boxed value. It is a strictly better target than the generic body, and it is still correct when the guard that just failed said nothing about the slots' representation.

The assertion is therefore re-pointed at that property, following #7492's worked example:

* the failure edge must reach a clone that does not assume raw-f64 slots (`$generic` **or** `$pshape`);
* it must **not** reach `$typed_f64_recv`, whose whole premise is the guard that just failed;
* and when it reaches `$pshape`, that clone must contain `js_number_coerce` — without which it would be the typed clone under another name.

Both new assertions were **sabotage-verified**: breaking the coercion check and narrowing the accepted callee set each make the test fail with its own message; restoring passes.

Because the suite is now fully green, its `SUITE_EXCLUSIONS` entry is deleted and `native_proof_regressions` joins `SOURCE_SUITE_MAP`. That is not optional bookkeeping — `e2e-scoped` fails the job when an excluded test PASSES, and `ci_e2e_scope.py --self-test` refuses a suite that is in neither list, which is what caught the half-done state while I was making this change. Self-test passes.

Worth recording alongside #7245: of the 16 tests that issue reported red, **15 had already been fixed by unrelated work** with nothing recording it. This was the only real one.
