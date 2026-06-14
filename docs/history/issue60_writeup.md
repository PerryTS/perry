# Issue #60: Dynamic Property Write Performance

## Problem

`obj["field_" + j] = value` was **84× slower** than Node.js (1,300ms vs 16ms for 10k objects × 20 fields).

## What we shipped (v0.5.51 + v0.5.55)

Two changes brought it to **77ms (4.5× Node)** — a **17× speedup**:

### 1. Content-hash transition cache (v0.5.51, `object.rs`)

The shape-transition cache was keyed on **string pointer identity**. Each `"field_" + j` allocates a fresh string, so the pointer differs across objects even though the content is identical. Every write missed the cache and fell through to the O(n) linear key scan + clone-on-shared slow path.

**Fix**: Key the cache on FNV-1a content hash instead of pointer. Same content → same hash → cache hit regardless of which allocation produced the string. Cache size increased 4096 → 16384.

**Result**: 1,300ms → 136ms (9.6× faster).

**Key code**: `object.rs` — `TransitionEntry.key_hash`, `key_content_hash()`, `transition_cache_lookup()`, `transition_cache_insert()`.

### 2. Static global cache + atomic descriptor flag (v0.5.55, `object.rs`)

The transition cache was a `thread_local!` with `UnsafeCell`. On macOS/aarch64, each `TRANSITION_CACHE.with(|c| ...)` call goes through the dyld TLV resolver, adding ~350ns per access. User code is single-threaded, so TLS is unnecessary.

**Fix**: `static mut TRANSITION_CACHE_GLOBAL` (plain global, no TLS). Also replaced `ANY_DESCRIPTORS_IN_USE` from `thread_local! Cell<bool>` to `static AtomicBool` with `Relaxed` load (single instruction, no TLS overhead).

**Result**: 136ms → 77ms (1.8× faster).

**Key code**: `object.rs` — `TRANSITION_CACHE_GLOBAL`, `GLOBAL_DESCRIPTORS_IN_USE`.

## What we tried and rejected

### Inline write PIC in codegen (two attempts)

**Idea**: Emit the transition cache lookup as inline LLVM IR at each `obj[key] = val` call site, bypassing the `extern "C"` function call overhead (pointer validation, GC header check, closure detection, frozen/sealed check).

**Attempt 1 — Hash helper function + inline cache check**: Called `perry_key_content_hash(key)` (small exported function doing just FNV), then did the cache slot computation + entry comparison + direct field write inline. **Result: 98ms (slower than 77ms)**. The hash function call had the same overhead as the validation it replaced.

**Attempt 2 — Fully inline FNV loop + cache check**: Emitted the FNV-1a byte loop as raw LLVM IR with phi back-edges, plus cache lookup and direct write — zero function calls on the hit path. **Result: 95ms (still slower)**. The inline code bloated the loop body, causing instruction cache pressure and branch predictor pollution. The pre-compiled runtime function is better optimized by Rust's compiler and shared across all call sites.

**Lesson**: Inlining doesn't help when the hash computation dominates the per-write cost. V8's inline caches work by *eliminating* the hash (interned strings → pointer identity check → 1 instruction), not by inlining it.

### Other micro-optimizations tried

- **Fast hash for short keys** (bulk u64 load + wyhash mix instead of byte-by-byte FNV): Slower — `copy_nonoverlapping` + extra branching was worse than the simple loop that LLVM already unrolls well.
- **Hoisting cache check before validation**: Slightly slower — extra conditional branching on the keys_array validation added overhead without saving enough on the (already-fast) frozen/sealed check.
- **Hybrid pointer+content cache**: Two-level lookup (try pointer identity first, fall back to content hash). Slower — checking two cache slots per write added overhead.

## Remaining gap analysis

**77ms Perry vs 17ms Node = 4.5×**

Per-write cost breakdown (200k writes):

| Step | Perry | V8 (Node) |
|------|-------|-----------|
| Function call frame | ~10ns | 0ns (IC inline in JIT code) |
| Pointer validation + GC/closure checks | ~20ns | 0ns (IC guarantees type) |
| FNV-1a hash (8 bytes) | ~10ns | 0ns (strings pre-interned) |
| Cache lookup (global array load + compare) | ~5ns | ~2ns (pointer compare) |
| Field write | ~5ns | ~2ns |
| **Total** | **~50ns** | **~4ns** |

String concatenation adds ~40ms to both (similar cost).

## What would close the gap

**Property-name string interning** — the one optimization that would actually eliminate the hash:

1. Add a global intern table: `HashMap<(u32, [u8; 8]), *const StringHeader>` keyed on (byte_len, first-8-bytes) with content verification
2. In `js_string_concat` and other string-producing functions, check the intern table before returning
3. First occurrence allocates normally; subsequent identical strings return the cached pointer
4. The intern table is a GC root (entries are never freed — property name strings are typically long-lived)
5. The transition cache switches back to **pointer identity** (one `usize` compare, no hash)

This would reduce per-write cost from ~50ns to ~15ns (3× faster), bringing Perry to ~25ms — within 1.5× of Node.

**Scope**: This touches the string allocator (`string.rs`), GC roots (`gc.rs`), and the transition cache (`object.rs`). The intern table lookup adds ~5ns to every string concatenation, but saves ~15ns on every property write. Net positive for property-heavy code, roughly neutral for string-heavy code without property access.

## File locations

- **Transition cache**: `crates/perry-runtime/src/object.rs` lines 396-490
- **`js_object_set_field_by_name`**: `crates/perry-runtime/src/object.rs` ~line 2220
- **IndexSet codegen**: `crates/perry-codegen/src/expr.rs` ~line 1964
- **Runtime declarations**: `crates/perry-codegen/src/runtime_decls.rs` ~line 856
- **StringHeader layout**: `crates/perry-runtime/src/string.rs` line 42 (utf16_len:0, byte_len:4, capacity:8, refcount:12, data:16)
- **ObjectHeader layout**: `crates/perry-runtime/src/object.rs` line 929 (object_type:0, class_id:4, parent_class_id:8, field_count:12, keys_array:16, fields:24+)
