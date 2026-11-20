### Allocation: the typed-shape layout is a property of the shape, so it is now a constant in the header

Four allocation benchmarks sat inside a **6% band** at 2.50–2.65× node — object literals
(`churn`, `churn_alloc`), class instances (`push_cls`), cyclic graphs (`cycles`). Four
structurally different shapes do not land in a 6% band by coincidence; one shared
per-allocation cost does.

Symbolicated profiles of the 200M-allocation variants (two samples per program, agreeing
within 1.5 pp) found it. `js_gc_declare_typed_shape_layout` was **30% of `churn_alloc` and
`push_cls`**, and it spent that re-deriving *per object* a fact that is a property of the
*shape*. #7510's memo had already collapsed the map round-trip to a direct-mapped probe;
what remained was the probe, a type-table lookup, a field-count compare, and the cross-crate
call itself. GC pause time, by contrast, is **3.5%** — the cost is that Perry performed nine
out-of-line operations per allocation where V8 performs a bump-pointer and a write barrier.

For a shape whose pointer mask is **statically empty**, the canonical layout is the constant
`GC_LAYOUT_POINTER_FREE | GC_OBJ_TYPED_LAYOUT_INTACT`. The inline-bump `new` path already
emits a packed `GcHeader` constant carrying the state half, so the intact bit is folded into
that same store and the call is not emitted. What survives is the one half that depends on
the recycled **address** rather than the shape — clearing whatever per-object record a
previous tenant left — as a one-argument `js_gc_forget_object_layout` behind an inline
`PERRY_PER_OBJECT_LAYOUTS_ANY` test. That global is a process-wide mirror of the per-thread
emptiness flag, maintained by an armed-thread count; its `0` state proves every thread's
per-object tables empty, and it is now also the first test inside `layout_forget_object`
itself, replacing a Darwin `_tlv_get_addr` call with a static load on the disarmed path for
every caller including object death.

Two smaller levers in the same band:

- **`js_ctor_return_override`** was called on every construction to answer a question that is
  `undefined` for every constructor without an explicit `return` — 8% of `churn_alloc`, where
  the synthesized object-literal constructor's only `ret` is the `TAG_UNDEFINED` constant.
  `JSValue::is_undefined` is `bits == TAG_UNDEFINED`, so one 64-bit compare decides it inline.
  The runtime call stays on the cold arm, where derived-constructor `TypeError`s, object
  returns, arguments objects and arrays still need it.
- **A `new` in a hot-loop callee is a `new` in a loop**, one frame out, so it now takes the
  inline bump as well. `cycles.ts`'s `makeCycle` is the shape that needed this: 5 statements,
  therefore `alwaysinline`, therefore *never* `inlinehint` — so the site gate was reading the
  one flag that could not be set for the hottest function in the program. The signal is
  `collect_hot_loop_callees` directly (≥1 in-loop call site AND ≤4 module-wide call sites),
  which is the same anti-bloat bound the loop arm already accepts.

Measured on the quiet M1 mini, best-of-5, with exit code 0 and byte-identical output verified
for all 27 corpus programs before timing:

| bench | before | after |
|---|--:|--:|
| `churn` | 0.4217 | **0.2900** (−31%) |
| `churn_alloc` | 0.3720 | **0.2409** (−35%) |
| `push_cls` | 0.3665 | **0.2368** (−35%) |

`churn_alloc` goes **18.6 → 12.0 ns per allocation**; node is 7.1 ns on the same shape.
Nothing else in the 19-benchmark corpus moves outside noise.

`gc-handoff/bench/alloc_declare_pf.ts` and `alloc_declare_ptr.ts` isolate the cause with a
control: the same program, same allocation count, same runtime stores, differing only in
whether the second field's *declared type* makes the pointer mask non-empty. Before, both
arms pay the declare and their times match (0.9044 / 0.8647); after, only the control does
(0.5802 / 0.7845). Both move by the shared return-override lever (1.6 ns/alloc); the extra
4.1 ns/alloc on the pointer-free arm is the layout declare itself.

**Soundness.** The collector's view is bit-identical: `heap_payload_slot_selection` skips a
`GC_LAYOUT_POINTER_FREE` payload without consulting any map, and the pre-existing path also
reached `POINTER_FREE` for an empty pointer mask. A later pointer store still downgrades —
with no descriptor to classify against, `layout_note_slot` falls through to its generic
pointer-mask branch, which mints a per-object mask and flips the state to `SIDE_MASK`, a
branch that needs no descriptor at all. A pointer-**bearing** shape keeps the full runtime
declare and must: `SIDE_MASK` means the tracer reads a mask, and that call is what installs
the shared `SHAPE_LAYOUTS` descriptor the mask lives in. (Installing it once at module init
is not a substitute: the `keys_array` lives in the longlived arena and can be relocated by
old-page defrag, and today's design survives that only by re-installing on the next
construction.)

One hypothesis was refuted rather than assumed: "INTACT set with no descriptor installed" is
*not* a wrong-answer hazard. A JS number's NaN box **is** its double bits, so the raw-f64
claim never changes the storage; every site that writes a slot raw re-proves the value finite
inline; and every read that could treat those bits as a machine double is itself
value-guarded. Verified directly — `(p as any).a = true` followed by `p.a + 1` through a
warmed monomorphic accessor prints node's `2` on both arms, byte-identical.

Tests: `crates/perry-codegen/src/lower_call/typed_shape_bake_tests.rs` — three IR-census
ratchets of the "assert the subject was live" kind, one per direction (pointer-free bakes,
pointer-bearing keeps the declare, `undefined` completion takes the inline arm). They earned
that description: the first version **failed**, because codegen had scalar-replaced the
probe's `new` and there was no allocation left to assert about. The escape is now explicit
and commented, for the next person who writes a probe on this path.
