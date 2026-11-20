### feat(native): add checked scalar conversions

`perry/native` now exposes checked `i32`, `i64`, `u32`, `u64`, `usize`,
`f32`, and `f64` value conversions alongside its fixed-width type aliases.
Conversions reject non-numeric, non-finite, fractional, out-of-range, or
precision-losing inputs with catchable errors, while `f32` rounds explicitly
to its representable value.
