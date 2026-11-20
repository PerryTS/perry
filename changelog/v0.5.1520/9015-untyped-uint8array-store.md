Stores through an untyped helper now preserve Buffer-backed `Uint8Array` values.

Perry's function inliner can specialize an `any`-typed index helper at a
`Uint8Array` call site and route the write through the generic typed-array
setter. That setter assumed every receiver had `TypedArrayHeader` layout, while
Perry represents `Uint8Array` with `BufferHeader`; the different data offsets
made the write disappear. The setter now validates the runtime receiver just
like the getter and dispatches Buffer-backed or reassigned receivers through
ordinary dynamic set semantics.

The Buffer path also performs ToNumber and Uint8 modulo narrowing before its
integer-only ABI, so NaN-boxed `any` values such as `257` and `-1` store as `1`
and `255` rather than `0`.
