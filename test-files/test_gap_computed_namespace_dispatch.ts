// #6677: a DIRECT computed-key method call on the Math / Object / JSON
// namespace objects (e.g. `Math["max"](1,2)`) threw
// `TypeError: value is not a function`. The call-dispatch arms in HIR lowering
// gated on `MemberProp::Ident` only, so a string-literal computed key
// (`Math["max"]`) never matched the namespace arm and fell through to generic
// dispatch, which dropped the namespace receiver and lowered the callee to an
// undefined global. The property READ (`const f = JSON["stringify"]`) and a
// dynamic/variable key (`Math[k]`) already worked; only the DIRECT
// string-literal computed call was broken. Minified/bundled output routinely
// mangles member access to the computed form, so `NS["m"]()` must match Node.

// --- Math ---
console.log(Math["max"](1, 2), Math["min"](3, 4), Math["floor"](3.7), Math["abs"](-5));
console.log(Math.max(1, 2), Math.min(3, 4), Math.floor(3.7), Math.abs(-5)); // dot form (regression guard)

// --- Object ---
console.log(Object["keys"]({ a: 1, b: 2 }).length);
console.log(Object["values"]({ a: 10, b: 20 }).join(","));
console.log(Object["entries"]({ a: 1 }).length);
console.log(Object.keys({ a: 1, b: 2 }).length); // dot form (regression guard)

// --- JSON ---
console.log(JSON["parse"]('{"x":9}').x);
console.log(JSON["stringify"]({ z: 4 }));
console.log(JSON.parse('{"x":9}').x); // dot form (regression guard)

// The read-then-call form was never broken — keep it green.
const f = JSON["stringify"];
console.log(f({ y: 3 }));

// NOTE: a *dynamic* computed key on these namespaces (`Math[k](...)` where `k`
// is a variable) is a separate, deeper limitation — perry has no runtime
// namespace object for Math/JSON/Object to index at runtime, so the receiver
// lowers to an undefined global. That is out of scope for #6677, which is
// specifically the DIRECT string-literal computed call, so it is not covered
// here.
