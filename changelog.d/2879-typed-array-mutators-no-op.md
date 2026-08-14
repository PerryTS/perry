Fixed `fill`, `reverse` and `copyWithin` silently doing nothing on a typed array
whose type is statically known. No error, no diagnostic, the array simply came
back unchanged:

```ts
const f = new Uint16Array(4); f[0]=1;f[1]=2;f[2]=3;f[3]=4
f.fill(9,0,2)            // was 1,2,3,4 — node gives 9,9,3,4
const r = new Uint16Array(4); r[0]=1;r[1]=2;r[2]=3;r[3]=4
r.reverse()              // was 1,2,3,4 — node gives 4,3,2,1
const c = new Uint16Array(4); c[0]=1;c[1]=2;c[2]=3;c[3]=4
c.copyWithin(0,2)        // was 1,2,3,4 — node gives 3,4,3,4
```

**Root cause — a delegation that stopped being reachable.** Codegen routes a
statically-typed typed-array receiver through the *generic* `js_array_*` helpers
on purpose (#3148/#654: `is_array_expr` in
`crates/perry-codegen/src/type_analysis/predicates.rs` answers `true` for
`Int32Array` &co.), on the contract that each helper re-dispatches on
`lookup_typed_array_kind`. Around forty helpers in `crates/perry-runtime/src/array/`
implement their half of that contract, and the four in-place mutators wrote
theirs *after* the shared `clean_arr_ptr` receiver funnel:

| helper | delegation was at |
|---|---|
| `js_array_fill` | `array/concat_reverse.rs` |
| `js_array_fill_range` | `array/concat_reverse.rs` |
| `js_array_reverse` | `array/concat_reverse.rs` |
| `js_array_copy_within` | `array/immutable.rs` |

Two later changes turned that ordering into dead code. #7574 made
`clean_arr_ptr` reject every *tracked non-`GC_TYPE_ARRAY`* object — correctly, a
`TypedArrayHeader`'s raw per-kind storage is not boxed-f64 `ArrayHeader` slots —
and the 2026-07-09 typed-array audit gave every typed array a real
`GC_TYPE_TYPED_ARRAY` header (they used to be header-less side-table
allocations, which the funnel let through). From then on all four mutators
returned at their `arr.is_null()` early-out, and the typed branch below it could
never run. The failure is invisible: `fill`/`reverse`/`copyWithin` all return
their receiver, so a no-op and a success are indistinguishable at the call site.

**The fix is the ordering, not the routing.** `clean_arr_ptr`'s rejection stays
exactly as #7574 left it. A new `typed_array_receiver` (`array/header.rs`)
answers "is this a registered typed array?" from the raw, possibly NaN-boxed
argument — a side-table probe that never dereferences the address — and each of
the four mutators consults it *before* the clean. Fixing it in codegen instead
(declining `is_array_expr` for typed arrays) was rejected: it would strand the
other ~40 delegations that the same contract depends on, and it would not fix
the `any`-typed receiver at all, because HIR's `copyWithin` fold
(`crates/perry-hir/src/lower/expr_call/local_array_methods.rs`) declines only
for a *known* typed array and otherwise lowers straight to the same runtime
helper.

`js_array_copy_within` additionally gained the Buffer/`Uint8Array` arm it was
missing, delegating to `object/buffer_dispatch.rs`'s byte-granularity
`copyWithin`. That shape reached the helper through the same HIR fold, so
`copyWithin` on an opaquely-typed `Uint8Array` was a no-op too, even though
every statically-typed `Uint8Array` case already worked.

**Verification.** A generated matrix of 5 programs × 66 checks —
`fill`/`reverse`/`copyWithin` × `Uint8Array`/`Uint16Array`/`Int32Array`/`Float64Array`/plain
`Array` × statically-typed / `any`-annotated / laundered-through-`any` receiver ×
function scope / module scope, with 0-, 1-, 2- and 3-argument `fill`, an
odd-length `reverse`, and negative plus out-of-range `copyWithin` indices — is
now byte-identical to `node` 26.5.1 on all 330 lines. 164 of them were wrong
before: 52 each for `Uint16Array`/`Int32Array`/`Float64Array` (every mutator,
every scope, every typedness) and 8 for `Uint8Array` (opaque `copyWithin` only).
The plain-`Array` program was correct before and after.
The unit coverage is `crates/perry-runtime/src/array/typed_array_receiver_tests.rs`:
it pins the precondition (`clean_arr_ptr` still rejects a typed array, so the
pre-check is load-bearing), asserts the plain-`Array` path is untouched, and
proves the *element-typed* store actually ran by filling values that only
survive per-kind truncation (`70000` → `4464` in a `Uint16Array`,
`2147483648` → `-2147483648` in an `Int32Array`) — so no test here can pass
merely by not throwing.

**Same class, still open.** `sort`, `with` and `toReversed` on a
statically-typed typed array are wrong for the same reason (`array/sort.rs`,
`array/immutable.rs` — post-clean delegations): `sort` leaves the array
unsorted, `with`/`toReversed` return an empty array. They are not part of this
change because each needs its own oracle matrix (comparator forms, and a
NEW-array return rather than in-place mutation).
