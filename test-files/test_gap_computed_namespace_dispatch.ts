// #6677: a DIRECT computed-key method call on a builtin namespace object
// (e.g. `Math["max"](1,2)`) threw `TypeError: value is not a function`. The
// call-dispatch arms in HIR lowering gated on `MemberProp::Ident` only, so a
// string-literal computed key (`Math["max"]`) never matched the namespace arm
// and fell through to generic dispatch, which dropped the namespace receiver
// and lowered the callee to an undefined global. The property READ
// (`const f = JSON["stringify"]`) and a dynamic/variable key already worked;
// only the DIRECT string-literal computed call was broken. Minified/bundled
// output routinely mangles member access to the computed form, so `NS["m"]()`
// must match Node. The fix widens the dispatch gate to accept the string-literal
// computed form for every builtin namespace (mirroring the `Symbol['for']`
// precedent), covering the three namespaces in the report plus the sibling
// ECMAScript builtins that shared the identical gate.

// --- Math / Object / JSON (the reported namespaces) ---
console.log(Math["max"](1, 2), Math["min"](3, 4), Math["floor"](3.7), Math["abs"](-5));
console.log(Math.max(1, 2), Math.min(3, 4), Math.floor(3.7), Math.abs(-5)); // dot form (regression guard)
console.log(Object["keys"]({ a: 1, b: 2 }).length);
console.log(Object["values"]({ a: 10, b: 20 }).join(","));
console.log(Object["entries"]({ a: 1 }).length);
console.log(Object.keys({ a: 1, b: 2 }).length); // dot form (regression guard)
console.log(JSON["parse"]('{"x":9}').x);
console.log(JSON["stringify"]({ z: 4 }));
console.log(JSON.parse('{"x":9}').x); // dot form (regression guard)

// --- Sibling ECMAScript builtins with the same gate ---
console.log(Number["isInteger"](5), Number["isNaN"](NaN), Number["isFinite"](5), Number["parseFloat"]("3.14"), Number["parseInt"]("42", 10));
console.log(String["fromCharCode"](65, 66, 67), String["fromCodePoint"](97));
console.log(BigInt["asIntN"](8, 300n), BigInt["asUintN"](8, 300n));
console.log(Array["isArray"]([1, 2]), Array["of"](1, 2, 3).join(","), Array["from"]("ab").join(","));
console.log(Reflect["ownKeys"]({ a: 1, b: 2 }).join(","), Reflect["has"]({ a: 1 }, "a"), Reflect["get"]({ a: 5 }, "a"));
console.log(Map["groupBy"]([1, 2, 3, 4], (x: number) => (x % 2 ? "odd" : "even")).get("even").join(","));
console.log(Date["UTC"](2024, 0, 1), Date["parse"]("2024-01-01T00:00:00Z")); // deterministic (no Date.now)
console.log(URL["canParse"]("https://a.com"));
console.log(ArrayBuffer["isView"](new Uint8Array(4)), ArrayBuffer["isView"]({}));
console.log(RegExp["escape"]("a.b*c"));

// The read-then-call form was never broken — keep it green.
const f = JSON["stringify"];
console.log(f({ y: 3 }));

// NOTE: a *dynamic* computed key on these namespaces (`Math[k](...)` where `k`
// is a variable) is a separate, deeper limitation — perry has no runtime
// namespace object for Math/JSON/Object/… to index at runtime, so the receiver
// lowers to an undefined global. That is out of scope for #6677, which is
// specifically the DIRECT string-literal computed call, so it is not covered
// here.
