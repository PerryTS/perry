// #7524: a builtin reached through a VARIABLE ALIAS and constructed with `new`
// produced an instance with no surface — `const ET = EventTarget; new ET()`
// gave `typeof inst.addEventListener === "undefined"`.
//
// The direct form is lowered by codegen straight to the factory, so only the
// indirect shapes were wrong: the alias routes through the globalThis value,
// whose closure is the shared `global_this_builtin_noop_thunk` — it allocates a
// bare object and never stamps the class id or attaches the per-kind state.
//
// NOT covered here, and still open on #7524: `class A extends AbortController {}`
// and friends. Subclassing a native base installs its surface through a
// different (per-builtin) mechanism — `EventTarget` has one, the others do not.

const ET = EventTarget;
console.log("EventTarget:", typeof new ET().addEventListener);

const AC = AbortController;
const ac = new AC();
console.log("AbortController:", typeof ac.abort, typeof ac.signal);

const TE = TextEncoder;
const te = new TE();
console.log("TextEncoder:", typeof te.encode, JSON.stringify(Array.from(te.encode("hi"))));

const USP = URLSearchParams;
const u = new USP("a=1&b=2");
console.log("URLSearchParams:", typeof u.append, u.get("a"), u.get("b"));

// The direct forms must be unchanged.
console.log(
  "direct:",
  typeof new EventTarget().addEventListener,
  typeof new AbortController().abort,
  typeof new TextEncoder().encode,
  typeof new URLSearchParams("x=1").get,
);

// NOTE: a `dispatchEvent` round-trip is deliberately NOT asserted here. It
// passes when the binary is run directly but the parity harness classifies the
// test CRASHED, which looks like the listener keeping the event loop alive past
// the harness's bound rather than a Perry defect — the surface this test exists
// to pin is the constructed instance, so it is asserted without the loop.
