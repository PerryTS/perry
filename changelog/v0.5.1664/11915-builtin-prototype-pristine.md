Method calls lowered by name now honour the method the receiver actually
reads (#11915). A replaced, deleted or redefined builtin prototype method —
`Array.prototype.push = f`, `Reflect.set(String.prototype, "slice", g)`,
`delete Array.prototype.indexOf`, `Object.setPrototypeOf(Map.prototype, p)`,
a replaced `Object.prototype.hasOwnProperty` — is called (or throws its
TypeError) on proven arrays, strings, Maps, Sets and Dates, on tag- and
kind-guarded receivers, in the universal dispatcher, and in borrowed
`Array.prototype.m.call(x)` / `Object.prototype.m.call(x)` calls. A saved
original called from a wrapper over it (`orig.apply(this, args)`) still runs
the builtin.

One proof serves every path: `PERRY_BUILTIN_PROTO_PATCHED`, one immortal
set-only byte per builtin prototype family, set by the object
mutation funnels once a sealed intrinsic prototype is written, defined,
deleted or relinked. The proof lives in the callee wherever it can: a
string method or folded Array method whose arguments cannot observe when the
method is read calls a checked entry (`js_string_m_<method><argc>`,
`js_array_m_<method><argc>`) that tests the byte — and, for an array, the
receiver's own-property and `[[Prototype]]` bits — before running the body
the site used to emit, so those sites are one call and smaller than before.
Guards a site already had absorb it at no cost: the own-override gate is
armed by every prototype modification and its predicate answers for it, the
inline `push`/`pop` admission's summary byte carries an "Array methods
modified" bit, and the inline `charCodeAt` admits a heap string only at or above
`PERRY_STRING_INLINE_MIN_HANDLE`, which rises past every address once `String.prototype`
or `Object.prototype` is modified. A call whose arguments could observe the
method read (a call, a property read) reads the byte before them, evaluates
them once, and calls the unchecked body (`js_string_b_*`, `js_array_b_*`) or
the generic path. A failure takes the one shared runtime path
(`js_builtin_proto_generic_call*` or the entry's own redirect), which
performs the real `Get` and call. No side table, name check or per-site
state. `PERRY_CHECK_CONTRACTS=1` emits a contract check on every admitted
direct arm that still tests at the site.

`PERRY_PROTO_VALIDITY` now also advances when a method held by any object
used as a prototype is replaced in place (same shape), so a memo proven by
`(ShapeId, PERRY_PROTO_VALIDITY)` cannot call a replaced method; the store
inline cache no longer learns a prototype's shape, so every such store
reaches the runtime funnel.

Builtin prototypes no longer carry their own copies of `hasOwnProperty`,
`isPrototypeOf`, `propertyIsEnumerable` and the Annex B accessor helpers;
they inherit `Object.prototype`'s. Map and Set property reads follow their
real prototype chain, and an unknown Map/Set method name is an ordinary
method call instead of a silent `undefined`.
