// Regression: `obj?.method(args)` (optional-chain on the RECEIVER, with a
// NON-optional call) must invoke the method, not short-circuit to undefined.
//
// SWC parses `s?.at(-1)` as a NON-optional OptChain call whose callee is an
// optional member (`s?.at`). The #4699 fix for the genuine optional-call form
// `foo?.bar?.(args)` added a function-value null-check guard, but that guard
// was being applied here too. For builtins (string/array methods) the callee
// `s.at` read as a bare PropertyGet is `undefined` — those methods only resolve
// through the call dispatch path — so the guard wrongly short-circuited the
// whole call and `s?.at(-1)` returned `undefined` instead of "/".
//
// The fix gates the function-value guard on `opt_chain.optional` (the `?.(`
// token): only the genuine optional-call form gets it; a non-optional call on
// an optional member becomes the plain `recv == null ? undefined : recv.m(a)`.

const s: string = "/";
console.log("at:", s?.at(-1) === "/");
console.log("slice:", "/x"?.slice(1) === "x");
console.log("charAt:", s?.charAt(0) === "/");

// null receiver must still short-circuit (no throw, returns undefined)
const n: any = null;
console.log("null:", n?.at(-1) === undefined);

// arrays
const arr = [1, 2, 3];
console.log("arr.at:", arr?.at(-1) === 3);
console.log("arr.map:", JSON.stringify(arr?.map((x) => x * 2)) === "[2,4,6]");

// number
console.log("num.toString:", (255)?.toString(16) === "ff");

// nested optional member then call
const o: any = { s: "hello" };
console.log("nested:", o?.s?.toUpperCase() === "HELLO");

// genuine optional-call form `foo?.bar?.(args)` still short-circuits (#4699)
const obj2: any = { method: undefined };
console.log("optcall short-circuit:", obj2.method?.(5) === undefined);
const obj3: any = { method: (x: number) => x + 1 };
console.log("optcall invoke:", obj3.method?.(5) === 6);

// Hono mergePath symptom: `root?.at(-1)` drives the double-slash decision
const root = "/";
const seg = "/healthz";
const merged = root?.at(-1) === "/" ? root.slice(0, -1) + seg : root + seg;
console.log("mergePath:", merged === "/healthz");
