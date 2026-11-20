**`Buffer.readFloatLE` and `readFloatBE` compile again.**

The NaN canonicaliser added for #10779 emits an `fcmp` on an `f32` lane, but `LlBlock::fcmp` rendered its operand type as `double` unconditionally and the in-process LLVM builder hardcoded the same. Any module calling `Buffer.readFloatLE` or `readFloatBE` therefore failed to compile with `'%r1' defined with type 'float' but expected 'double'`.

`LlInst::FCmp` now carries its operand type. `fcmp()` keeps its `double` signature and delegates to a new `fcmp_ty()`, so no existing call site changes.

Also closes the remaining raw float source: NaNs arriving from native code. A C `float` return previously segfaulted — the forged `StringHeader*` was dereferenced — and a C `double` return printed its payload integer. The `double` arm is gated strictly on the manifest declaring `F64`, because it also serves perry's own double ABI where the value already *is* a NaN box.
