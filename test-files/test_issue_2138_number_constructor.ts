// Refs #2138: property access on a primitive number receiver must
// auto-box for `.constructor` so `n.constructor === Number` matches
// Node. Pre-fix the #2128 guard correctly returned `undefined` for any
// key on a primitive-number receiver — strictly better than the prior
// SIGSEGV, but still off-spec for the inherited `constructor` lookup
// that lodash / date-fns use to discriminate primitive values via
// `value.constructor.name`.
//
// The runtime field-getter slow path now intercepts `b"constructor"`
// inside the same primitive-number guard and resolves it through the
// shared `js_get_global_this_builtin_value` helper that backs bare
// `Number` identifier lookups — same pointer either way, so the
// `=== Number` identity check holds. Unknown keys still return
// undefined (regression-tested below to confirm #2128 is preserved).

function asAny(v: any): any { return v; }

// Integer-valued primitive (NaN-boxed as an int32-tagged double under
// the hood — the small-int fast path).
const n = asAny(1);
console.log(n.constructor === Number);
console.log(typeof n.constructor);

// Float primitive.
const f = asAny(3.14);
console.log(f.constructor === Number);

// Zero — distinct bit pattern (f64 +0.0 = u64 0).
console.log(asAny(0).constructor === Number);

// Negative.
console.log(asAny(-42).constructor === Number);

// Unknown keys still return undefined (preserves #2128).
console.log(n.unknownKey === undefined);
console.log(asAny(5).foo === undefined);
