perf(codegen): inline checked-f64 typed-array-param reads in numeric context

A typed-array element read through a parameter that feeds arithmetic (`n += S[i]`,
the bcryptjs `_encipher` S-box shape) now lowers to an inline checked f64 load —
the same guard/bounds machinery as the checked-i32 read, widened to f64 and
bit-exact with `js_typed_array_get` (the `TAG_UNDEFINED` double on OOB) — instead
of a per-read runtime call. Guard misses defer to a new memory-safe
`js_typed_array_read_f64` helper. Gated on a proven non-negative integer index;
covers every numeric kind incl. Uint32 (unsigned widening) and the float kinds.
Env flag `PERRY_TA_PARAM_F64_READ` (default on). Measured ~1.32× on the real
`_encipher` shape (1787ms → 1351ms, byte-exact); stacks with the #6860 non-BigInt
inline-bitwise fast path.
