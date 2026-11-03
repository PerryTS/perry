### Fixed

**A reassigned `new Int32Array` binding made every element READ return `0`** (#8100)

```ts
let P: Int32Array = new Int32Array(1);
P[0] = 123;
console.log("ta:", P[0], P.length);
P = [99, 101] as any;
console.log("plain:", P[0], P[1], P.length);
```

```
perry (before):  ta: 123 1     plain: 0 0 2
node v26.5.1:    ta: 123 1     plain: 99 101 2
perry (after):   ta: 123 1     plain: 99 101 2
```

Exit status was 0 on both sides and `P.length` was already correct — the
binding really did hold the plain array. Only the element reads were wrong,
silently, in the shipped default configuration. All five representation-
selection kill switches (`PERRY_SPECIALIZED_ABI`, `PERRY_PTR_NUMARRAY_LOCALS`,
`PERRY_PTR_SHAPE_LOCALS`, `PERRY_INT_VALUED_LOCALS`,
`PERRY_CANONICAL_I32_LOCALS`) left it broken, which is why
`scripts/gc_repsel_matrix.sh` failed the `specabi_reassign` row in 22 of 22
arms including `shipped_default`, and why `gc-stress` was red on every `main`
nightly since 2026-08-10.

#### Root cause: the receiver was validated, but the answer was not dispatched

`perry-codegen`'s `is_width_tracked_typed_array_receiver`
(`expr/index_get.rs`, #7494) deliberately keeps a local's DECLARED typed-array
kind even after the binding is reassigned. That is the right call — dropping
the hint sends a REAL typed array on to `is_array_expr`'s plain-array layout
(element 0 at byte 8 instead of the data region at byte 16), which is a
type-confused *write*, not merely a missed optimization. #7494 pays for the
hint with an explicit promise, written in its comment: the runtime helper
"re-validates the object's actual GC kind before touching memory".

`js_typed_array_get` did not. Its only receiver check was `clean_ta_ptr`,
which rejects nothing but an address below `0x1000`. So a plain array was read
**as** a `TypedArrayHeader`:

* `TypedArrayHeader::length` and `ArrayHeader::length` are both `u32` at
  offset 0, so the bounds check passed against the plain array's real length;
* `kind` (offset 8) and `elem_size` (offset 9) came from the low two bytes of
  element 0's NaN box — for `99.0` both are `0`, i.e. `KIND_INT8` with a zero
  stride;
* `data_ptr(ta)` is `ta + size_of::<TypedArrayHeader>()` = `ta+16`, which is
  element **1** of a plain array (whose slots start at `ta+8`).

The uniform `0` was a memory read, not a constant: with a plain *object*
receiver the same expression returned `8`.

#### Fix: `classify_element_read_receiver`, asked before anything dereferences

New `classify_element_read_receiver` (`typedarray/mod.rs`) is the READ-side
mirror of #8090's `array/header.rs` `typed_array_receiver`: it answers from the
raw, tag-masked argument **before** `clean_ta_ptr`.

* A registered %TypedArray% keeps the typed element path unchanged. A
  `GC_TYPE_TYPED_ARRAY` / `GC_TYPE_NATIVE_TYPED_VIEW` managed header also wins,
  so a registry miss can only cost the diversion, never the element read.
* Anything else takes the ordinary `[[Get]]` (`js_dyn_index_get`). Codegen
  masks the NaN-box tag off the receiver (`and i64 %bits, POINTER_MASK`), so
  the tag is RECONSTRUCTED from the managed header — which matters for exactly
  one case: a heap string must be re-boxed `STRING_TAG` or `js_dyn_index_get`
  walks a `StringHeader` as an `ObjectHeader` instead of taking its string arm.
  (Symbols share `GC_TYPE_STRING` and stay POINTER-tagged, which is what
  `js_is_symbol` separates.)
* A masked-away non-pointer (`P = 42 as any`) answers `undefined`, node's
  answer, instead of the old `0.0`.

Applied to both READ helpers: `js_typed_array_get` (constant index) and
`js_typed_array_index_get_dynamic` (variable / string key, via
`typedarray_props::typed_array_index_get_dynamic`, generalizing the #5989
buffer-only fallback that already sat there). The STORE side was already
correct — codegen's store path consults `ctx.buffer_view_slots`, which
invalidates on assignment, so a reassigned binding never reaches
`js_typed_array_set`.

#### Verified

`node --experimental-strip-types` at the `.node-version` pin (v26.5.1), every
exit code checked. An 11-section probe covering constant-index reads,
variable-key reads, canonical string keys, constant and dynamic stores, and
receivers that are a plain array / plain object / string / number / a real
typed array diverged from node on 9 lines before and is byte-identical after.
`test-files/test_gap_specabi_reassign.ts` is byte-identical to node.

10 unit tests in
`crates/perry-runtime/src/typedarray/element_read_receiver_tests.rs`. They
assert element VALUES, not absence of a panic; the typed-array controls store
`70000` into a `Uint16Array` and require `4464` back, so a fallback that
hijacked the typed path into boxed-f64 slots fails them. Sabotage-verified:
with the two dispatch call sites reverted and the tests unchanged, 4 of the 10
go red.
