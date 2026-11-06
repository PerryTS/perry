// Issue #1069: cross-module `arguments` bundling for Effect's `dual()` pattern.
//
// `duallib` exports `hasProperty = dual(2, body)` — a closure stored as a
// const, whose body reads `arguments.length` to discriminate data-first vs
// curried. The previous PR series (#915 / gap 1) made the LOCAL call site bundle
// ALL args for synthetic-`arguments` rest params. But the CROSS-MODULE call
// site in `lower_call.rs` (the `ExternFuncRef` path that emits
// `perry_fn_<src>__<name>(...)`) still only bundles TRAILING args beyond
// fixed_count — matching ordinary user `...rest`, not the synthetic-arguments
// case. So `hasProperty({}, "x")` cross-module reaches its body with
// `arguments.length === 0` (only zero trailing args) instead of `2`.
import { hasProperty, getArity } from "duallib";

// Data-first: 2 args → boolean
console.log("hasProperty 2-arg:", hasProperty({ x: 1 }, "x"));  // expect: true
console.log("hasProperty 2-arg miss:", hasProperty({ x: 1 }, "y"));  // expect: false

// Curried: 1 arg → function
const isX = hasProperty("x") as any;
console.log("hasProperty curried typeof:", typeof isX);  // expect: function
console.log("hasProperty curried apply:", isX({ x: 1 }));  // expect: true

// Plain arity probe (not dual, just an exported `function` reading arguments).
console.log("getArity 0:", getArity());        // expect: 0
console.log("getArity 1:", getArity("a"));     // expect: 1
console.log("getArity 3:", getArity(1, 2, 3)); // expect: 3
