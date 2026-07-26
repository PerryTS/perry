### Changed

- Lower two integer-math JS primitives that were compiled as runtime function
  calls to native machine ops, closing an ~9x AOT-vs-JIT gap on hot integer
  kernels (hashes/PRNGs/mixers/ciphers):
  - **`Math.imul(a, b)`** now lowers to a single native `mul i32` whenever both
    operands are provably in-range i32 (multiplication mod 2^32 has identical
    low 32 bits for signed and unsigned operands, so this is exact).
    Non-finite / fractional / `>2^32` operands keep the `js_math_imul` runtime
    helper so JS `ToUint32`/`ToInt32` semantics (`NaN`/`±Infinity` -> 0) are
    preserved. This also fixes the accumulator case `a = Math.imul(a, K)` where
    the constant `K` exceeds `i32::MAX` (e.g. the golden-ratio mixer constant
    `0x9e3779b1` = 2654435761): the i32 fast path now accepts integer literals
    representable in 32 bits under either a signed or unsigned interpretation.
  - **Reading a typed-array element through a parameter** (`S[i]` where
    `S: Int32Array` etc. is a function parameter, in an i32/`ToInt32` context)
    now lowers to a checked inline native load — a runtime guard (pointer +
    inline-storage `PERRY_TA_VIEW_GUARD` + kind-cache) and a header-length
    bounds check gate a bare width-correct load, an in-kind out-of-bounds read
    yields `0` (`== ToInt32(undefined)`, the only observable value in that
    context), and every rejected shape (view/detached/resizable backing, wrong
    runtime kind) defers to the new `js_typed_array_read_int32` runtime
    fallback. Perry already emitted bare loads for typed-array *locals* with
    proven bounds (#6750); this extends the recognition to *parameters*, whose
    length and storage are unknown at compile time. Plain-value parameter reads
    still observe `undefined` out of bounds.
  - On a 40M-iteration `Int32Array`-parameter bit-mixer that combines both
    primitives, the two fallbacks previously cascaded the whole hot loop into
    slow f64 `ToInt32` towers (`js_math_imul` x3, `js_typed_array_get` x3,
    ~60 `sitofp`/`fptosi`/`select`); both runtime-call families now reach zero
    and the read/multiply chain stays in native i32.
