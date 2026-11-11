// Closes #645 — chained method calls on a value that fell through
// `js_native_call_method`'s catch-all used to crash with
// `TypeError: (number).<method> is not a function`. The runtime
// returned a literal `0.0` from the `is_valid_obj_ptr` reject path,
// which the codegen interprets as IEEE-754 number zero — so the next
// chained call saw a number receiver and the dispatcher's primitive-
// receiver TypeError fired.
//
// Repro shape (drizzle's `this.stmt.raw().all(...params)` boiled down
// to its essentials): a chained method call where the receiver of
// each step is the result of the previous step. With Perry's existing
// "fall through to NULL_OBJECT_BYTES stub" semantics for unknown
// methods, every step in the chain must produce a `typeof === "object"`
// value — not a number — so the chain doesn't crash mid-way.
//
// Acceptance: the program runs to completion. Pre-fix it crashed at
// the second `.method()` with `(number).method is not a function`.
// We don't compare byte-for-byte with Node here because Node throws
// at the FIRST `.method()` (its standard semantics); Perry's silent-
// fall-through behavior is documented and tracked separately as part
// of the unimplemented-API surface (#648 / #463).

const obj: any = {};
const r1 = obj.nonExistentMethodA();
const r2 = r1.nonExistentMethodB();
const r3 = obj.a().b().c();

// Print a sentinel so a successful run is visibly distinguishable from
// the pre-fix crash (the crash printed nothing to stdout before exit).
console.log("ok r1=", typeof r1, "r2=", typeof r2, "r3=", typeof r3);
