**GC: `this` could become a stale from-space pointer after a moving collection inside a method body (#8495).**

A method whose body triggered an evacuating collection saw `this` become a dangling from-space pointer. Reads through it returned garbage — `Object.keys(this)` came back `[]` and `Object.getPrototypeOf(this) === Object.prototype` was `false` — so a method that collected before touching `this` silently observed an empty object:

```ts
const recv: any = { n: "RECV" };
recv.f = function (): string { churnGc(); return JSON.stringify(Object.keys(this)); };
recv.f();   // was: []   now: ["n","f"]
```

**Cause.** Twenty-two receiver save/restore pairs manipulated the `IMPLICIT_THIS` cell *directly* — `let prev = IMPLICIT_THIS.with(|c| c.replace(recv))` … call … `IMPLICIT_THIS.with(|c| c.set(prev))` — instead of going through `js_implicit_this_set`. The `replace` has already overwritten the cell, so the displaced receiver lives only in that local across the user call. An evacuating collection inside the callee moves the object and rewrites the cell (its scanner works correctly), and the restore then publishes the pre-move address back *into* the scanned cell. This is the same defect #7211 fixed on the codegen side with the `implicit_this_save`/`implicit_this_restore` combinator, and the rooted form already existed in-tree at `native_call_method/common_methods.rs`; these twenty-two sites had not been converted.

Each now roots the displaced receiver in a `RuntimeHandleScope` and restores by re-reading the handle.

**Why it hid.** Three things mask it, all worth knowing when auditing this area. `===` reports `true` for a moved-away pointer because equality resolves forwarding while ordinary reads do not, so identity checks look healthy. Reading any property of `this` *before* the collection makes later reads succeed. And because these sites bypass `js_implicit_this_set`, instrumenting that function shows no write at all — the cell appears to change on its own.

Found with `PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`, which faults at the stale use and names the retiring minor.

Closes #8495. `gc_property_key_operand_rooting_6935` goes 1/3 → 3/3.
