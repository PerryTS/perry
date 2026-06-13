# Performance Roadmap: Closing the Zig Gap

## Current State (v0.5.58)

Perry compiles TypeScript to native via LLVM. On the image_conv benchmark (5×5 Gaussian blur, 3840×2160 RGB):

| Component | Perry | Zig | Gap | Root Cause |
|---|---|---|---|---|
| Blur kernel | 280ms | ~200ms | 1.4× | No NEON autovectorization |
| Input gen (xorshift + gradient) | 120ms | ~30ms | 4× | NaN-box unbox per buffer access |
| FNV-1a hash | 57ms | ~16ms | 3.5× | Double-ABI function call overhead |
| **Total** | **457ms** | **246ms** | **1.86×** | |

Starting point was 1,980ms (8× Zig). We've done 4.3× improvement. The remaining 1.86× is architectural.

## What's Already Optimized

These are DONE and working — don't re-implement:

1. **i32 accumulator fast path** (`lower_expr_as_i32` in expr.rs): `rAcc += src[idx] * k` stays in i32. Handles Add/Sub/Mul/BitAnd/BitOr/BitXor/Shl/Shr/UShr, LocalGet (i32 slot or integer_locals), Integer, Uint8ArrayGet, flat-const IndexGet, clamp calls (smin/smax), Math.imul. The `can_lower_expr_as_i32` gate checks all leaves; `lower_expr_as_i32` emits native i32 ops.

2. **Flat `[N x i32]` const tables** (flat_const_arrays): `const KERNEL: number[][] = [[1,4,7,4,1],...]` → private unnamed_addr `[25 x i32]` in .rodata. `IndexGet(IndexGet(X, i), j)` and aliased `krow[j]` (via `array_row_aliases`) emit `getelementptr + load i32`.

3. **`@llvm.assume` bounds** in Uint8ArrayGet/Set: eliminates the branch+phi diamond for bounds checks. Single basic block per access.

4. **`sdiv` for `(int / const) | 0`**: Pattern-matched before generic BitOr lowering. LLVM converts to `smulh + asr`.

5. **`toint32_fast`**: Skips the 5-instruction NaN/Inf guard from v0.5.49 when `is_known_finite(ctx, expr)` returns true.

6. **`x | 0` / `x >>> 0` noop**: When left operand is known-finite and right is Integer(0), emit just `fptosi + sitofp` (no toint32 guard, no or/lshr).

7. **Clamp detection** (`detect_clamp3`, `detect_clamp_u8`): Threaded through `is_int32_producing_expr`, `collect_integer_let_ids`, `can_lower_expr_as_i32`. Call sites emit `@llvm.smax.i32 + @llvm.smin.i32`.

8. **`alwaysinline`** on functions ≤8 stmts and i64-specialized wrappers (`force_inline` field on LlFunction).

9. **`!invariant.load`** on buffer/array length loads.

10. **`returns_integer` detection**: Functions whose ALL return paths end with `| 0` / `>>> 0` / bitwise → included in integer-candidate seeding.

## The Three Optimizations That Would Close the Gap

### 1. Typed Buffer Locals (eliminates NaN-box unbox — biggest single win)

**Problem**: Every `src[idx]` and `dst[idx] = val` does:
```llvm
%handle_bits = bitcast double %buf_nanboxed to i64
%handle = and i64 %handle_bits, 0x0000FFFFFFFFFFFF  ; strip NaN-box tag
; ... then compute address from handle
```
That's 2 extra instructions per access × 75 accesses per pixel × 8.3M pixels = 1.24 billion wasted instructions.

**Fix**: When a `const buf = Buffer.alloc(N)` or function param is statically typed as `Buffer`/`Uint8Array`, store the raw `i64` pointer in an `i64` alloca instead of a NaN-boxed `double` alloca. `Uint8ArrayGet`/`Set` then skip the unbox — just `load i64` from the slot and use directly.

**Implementation sketch**:
- In `stmt.rs` `Stmt::Let`, detect `init: BufferAlloc { .. }` or type `Named("Buffer")`/`Named("Uint8Array")`. Allocate an `I64` slot instead of `DOUBLE`. Store the raw pointer (from `js_buffer_alloc` which returns `I64`) directly.
- In `Uint8ArrayGet`/`Uint8ArraySet`, check if the `array` expr is `LocalGet(id)` where `id` has a typed-buffer slot. If so, `load i64` from the slot directly — no `bitcast + and POINTER_MASK`.
- Track typed-buffer locals in a `HashSet<u32>` on `FnCtx` (like `i32_counter_slots`).
- Module globals that are buffers: store as `I64` global instead of `DOUBLE`.

**Estimated impact**: Eliminates ~2 instructions per buffer access. For input gen (48 accesses per 4-byte iteration × 6.2M iterations) + blur (75 accesses per pixel × 8.3M pixels), saves ~1.5B instructions → ~40-60ms.

### 2. Interior/Border Loop Splitting (enables NEON autovectorization)

**Problem**: The blur loop processes ALL pixels uniformly, including edge pixels that need clamping. The clamp logic (even as smin/smax) adds data-dependent index computation that prevents LLVM from vectorizing across pixels.

**Fix**: Split the y-loop into three regions:
- Top border (y = 0..1): clamp needed
- Interior (y = 2..H-3): no clamp needed, all indices guaranteed in-bounds
- Bottom border (y = H-2..H-1): clamp needed

Same for x. For the interior (99.9% of pixels), the kernel access pattern becomes:
```
idx = ((y + ky) * W + (x + kx)) * 3
```
— pure arithmetic, no clamp, no smin/smax. LLVM can then vectorize the x-loop with NEON `ld3`/`st3` for stride-3 RGB deinterleaving.

**Implementation**: This is a HIR-level transform, not codegen. Add a pass in `perry-transform` that detects the blur-like pattern:
```
for (y) for (x) for (ky) for (kx) {
  yy = clamp(y+ky, 0, H-1)
  xx = clamp(x+kx, 0, W-1)
}
```
And splits it into border + interior loops. The interior loop has the clamp calls replaced with direct arithmetic.

**Estimated impact**: Interior loop becomes vectorizable → 4× throughput on the accumulation. Blur drops from 280ms to ~80-100ms.

### 3. HIR-Level Function Inlining for Small Pure Functions

**Problem**: `clampIdx` and `imul32` are compiled as separate LLVM functions with double-ABI wrappers. LLVM's `alwaysinline` inlines them, but the `sitofp/fptosi` conversion chain at the call boundary persists in the IR until instcombine runs — and instcombine doesn't always collapse `fptosi(sitofp(select(icmp(...))))`.

**Fix**: Inline at the HIR level, BEFORE codegen. A pre-codegen pass in `perry-transform` that:
1. Identifies small (≤8 stmt) non-recursive pure functions.
2. At each call site, substitutes the function body with parameter renaming.
3. The result: the function body's `if/return` pattern is in the CALLER's HIR, and Perry's codegen sees it directly — no function call boundary, no double wrapper.

For `clampIdx(v, lo, hi)`, the inlined HIR becomes:
```
Let { yy_temp, init: v }
If { v < lo } → LocalSet(yy_temp, lo)
If { v > hi } → LocalSet(yy_temp, hi)
// yy_temp is now the clamped value
```

The codegen's `is_int32_producing_expr` already handles `If/Return` patterns and `LocalGet`/`LocalSet` — so the inlined body stays in the i32 path without any double conversion.

**Implementation**: Add a new pass in `crates/perry-transform/src/inline.rs`. Walk all `Call(FuncRef(id), args)` in the HIR. If the callee has ≤8 stmts, is not recursive, and is not async/generator, replace the Call with an inlined copy of the body (fresh LocalIds via the module's id allocator, parameter locals initialized from the call args).

**Estimated impact**: Eliminates the `sitofp/fptosi` chain at every clampIdx/clampU8/imul32 call site. For the blur (50 clamp calls per pixel × 8.3M pixels = 415M conversions eliminated) + FNV (24.8M imul32 call conversions eliminated). Combined: ~30-50ms saved.

## Priority Order

1. **Typed Buffer Locals** — broadest impact, affects ALL buffer-heavy code, relatively simple codegen change
2. **HIR-Level Inlining** — eliminates the double-ABI tax for ALL small functions, reusable across benchmarks
3. **Interior/Border Splitting** — most complex, biggest single-benchmark win, enables NEON

With all three: projected **200-250ms** total, matching Zig.

## Key Files

- `crates/perry-codegen/src/expr.rs` — `lower_expr`, `lower_expr_as_i32`, `can_lower_expr_as_i32`, `Uint8ArrayGet`/`Set`, `FnCtx` struct
- `crates/perry-codegen/src/stmt.rs` — `Stmt::Let` lowering, i32 slot allocation
- `crates/perry-codegen/src/collectors.rs` — `collect_integer_locals`, `is_int32_producing_expr`, `collect_integer_let_ids`, clamp/returns-integer detectors
- `crates/perry-codegen/src/codegen.rs` — `compile_function`, `compile_module_entry`, `CrossModuleCtx`, i64-specialization
- `crates/perry-codegen/src/block.rs` — `LlBlock` instruction emission (`toint32`, `toint32_fast`, `load_invariant`, `sdiv`, etc.)
- `crates/perry-codegen/src/function.rs` — `LlFunction`, `force_inline`, `to_ir`
- `crates/perry-transform/src/` — HIR transform passes (new inline pass would go here)
- `crates/perry-hir/src/ir.rs` — HIR data structures (`Expr`, `Stmt`, `Function`)
- `crates/perry-runtime/src/value.rs` — NaN-boxing constants and value representation

## Testing

- `benchmarks/honest_bench/workloads/3_image_convolution/perry/image_conv.ts` — the blur benchmark
- Timed variant: use `Date.now()` around each phase (inputgen/blur/fnv) for per-component measurement
- `/tmp/run_gap_tests.sh` — gap test suite, verify no regressions
- Correctness: `checksum=2ba2e053` for the standard 3840×2160 image
