**A property read whose shape already records the field as numeric now vouches as canonical raw f64.**

`expr_produces_canonical_raw_f64` refused every `PropertyGet` on principle, so an arithmetic leaf reading such a field kept a per-iteration tag test — even though the fact was present and already consumed by codegen, which emits `load(DOUBLE, …)` with `rep: F64`. It was then re-derived *syntactically* from the Expr node at the add. That is also why `o.a * 1` reaches 9 where `o.a` does not: `Binary` is a shape the predicate recognises, and the multiply normalises nothing.

`h += o.a` goes 35 → 33, `h += o.a + o.b` 50 → 43, `o.a = o.a + 1; h += o.a` 97 → 94 — 2–7 instructions per iteration where the receiver is a class instance with proven provenance.
