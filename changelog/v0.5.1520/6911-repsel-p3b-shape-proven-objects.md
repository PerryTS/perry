perf(codegen): representation-selection Phase 3b — shape-proven object locals (`Ptr<Shape>`)

For a function-local proven to hold exactly one `new C(...)` object with a
statically-immutable shape (provenance + containment + `this`-flow + dispatch
stability, with a module-wide first-increment kill on any
defineProperty/delete/setPrototypeOf/Proxy/mutating-Reflect site), field
accesses lower to the bare fixed-offset form — no per-access guard diamond, no
volatile gate, no fallback arm, no phi — and method calls dispatch directly
with no shape guard. Anon-shape record literals and extends chains qualify;
the typed-receiver f64 method clone is widened from extends-free classes to
fully-modeled chains with chain-global field indexes. Raw-f64 stores keep the
plain-finite check with a boxed-setter downgrade side exit; boxed stores keep
the generational write barrier; the local's slot stays a tagged-at-rest,
shadow-bound GC root (raw pointers never stored at rest; the mark/rewrite
raw-asymmetry is tracked as #6910). Gated by `PERRY_PTR_SHAPE_LOCALS`
(default on, object-cache keyed). Implements Phase 3b of
`docs/representation-selection-rfc.md`.
