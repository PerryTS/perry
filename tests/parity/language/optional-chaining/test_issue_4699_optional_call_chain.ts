// Regression for #4699: a chained optional call `foo?.bar?.(args)` must
// short-circuit when the *function value* (`foo.bar`) is null/undefined,
// not only when the receiver (`foo`) is. Pre-fix, the lowering checked
// only the receiver, so `def?.error?.(5)` (with `def.error === undefined`)
// invoked `undefined(5)` and threw "X is not a function" — the crash that
// blocked zod v4 `safeParse` on the validation-failure path.

// Receiver present, function value missing -> short-circuit to undefined.
const def: any = { foo: 1 };
console.log("a:", def?.error?.(5)); // undefined

// Function value present -> the call runs.
const obj: any = { greet: (n: string) => `hi ${n}` };
console.log("b:", obj?.greet?.("x")); // hi x

// Receiver missing -> inner ?. short-circuits.
const nul: any = null;
console.log("c:", nul?.error?.(5)); // undefined

// Deep chain mirroring zod's `iss.inst?._zod.def?.error?.(iss)`.
const iss: any = { inst: { _zod: { def: {} } } };
console.log("d:", iss.inst?._zod.def?.error?.(iss)); // undefined

const iss2: any = { inst: { _zod: { def: { error: (x: any) => `got ${x.v}` } } } };
console.log("e:", iss2.inst?._zod.def?.error?.({ v: 7 })); // got 7

// Missing intermediate link in the deep chain still short-circuits.
const iss3: any = { inst: null };
console.log("f:", iss3.inst?._zod.def?.error?.(iss3)); // undefined

// Computed-key variant: `foo?.[k]?.(args)`.
const tbl: any = { handler: undefined };
const k = "handler";
console.log("g:", tbl?.[k]?.(1)); // undefined

console.log("Optional-call chain tests passed!");
