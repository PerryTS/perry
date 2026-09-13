Make the element-shape versioned loop clone (#7480 / #5093) fire for
`JSON.parse`'d record arrays. Loops of the shape
`for (let i = 0; i < count; i++) sum += rows[7].id` and
`for (let i = 0; i < count; i++) { const index = i % length; sum += rows[index].id; }`
over an `any`-typed parsed array now run in the call-free fast clone instead of
the generic element-read + field-read diamonds.

The clone keyed every proof on a compile-time class, which structurally
excluded the case record-processing code is written against: a parsed record is
`class_id == 0` with an ordinary birth ShapeId, and `rows: any` resolves to no
class at all. Four separate things had to change, each of which independently
kept the clone dark:

* **Runtime.** `array/element_shape.rs` refused `class_id == 0` outright, so no
  parsed record array could ever carry an element-shape proof. Class 0 is now
  admitted keyed on the exact ordinary ShapeId — strictly narrower than the
  class-level identity, and necessarily so, since "same class" is vacuous when
  the class is 0. New FFI: `js_array_ensure_element_shape_ordinary` (the
  ShapeId for a class-0 proof, 0 for a class-keyed one — the two are
  deliberately not interchangeable) and `js_shape_ordinary_inline_slot_for_key`
  (the inline slot a plain ordinary shape assigns to a key, or -1). The key
  crosses as the whole NaN-box, not a masked pointer, because a short property
  name reaches the string pool as an SSO immediate whose masked low bits are
  packed characters rather than an address.
* **The brand test ran before the head repair.** `JSON.parse` of a top-level
  array in [1 KB, 16 MB] hands back a `GC_TYPE_LAZY_ARRAY` header, which the
  preheader's `GC_TYPE_ARRAY` brand rejected — before the repair step that
  would have materialized it. The repair now runs first and the brand is
  applied to the repaired head. `js_array_refresh_local_head` is safe on an
  unbranded value by construction: it resolves through `clean_arr_ptr`, which
  returns null for every tracked non-array.
* **The residual per-element check required `GC_OBJ_TYPED_LAYOUT_INTACT`.** A
  parsed record never has that bit — `object/json_construction.rs` finishes
  every record with `layout_init_pointer_free` or `layout_mark_unknown`, and
  both clear it — so the clone would have been emitted, entered, and then
  side-exited on the first element of every loop. The shape-keyed residual
  drops that conjunct and buys the same claim per read, from the value: the
  loaded word is tag-tested as a Number and a string / boolean / null `id`
  side-exits to the slow clone.
* **The index had to be the counter.** `rows[7]` and
  `const d = i % n; rows[d]` are now admitted, each with its own preheader
  bounds obligation (`length > k`, `1 <= m <= length`) so the clone still pays
  no per-read bounds test. The derived binding is virtual inside the clone —
  its `Let` emits one `srem i32` rather than the generic `%` lowering, which is
  a runtime call and would delete the clone rather than slow it. The matcher
  admits exactly one index form per loop.

The revocation argument is unchanged: it never mentioned classes, and every
funnel that retires a class-keyed proof retires a shape-keyed one. Call-free
is still the whole admission test, enforced by the matcher and by the
post-emission scan of every block the clone owns.

Measured with `benchmarks/json_performance/.work/fixtures/records_array_*.json`
and a `rows: any` access worker, five interleaved rounds, best of five, on one
compiler binary per arm. Checksums agree across all four engines.

| cell | before | after | node 26.5.1 | bun 1.3.14 |
|---|---|---|---|---|
| 16k repeat | 5.98 | **4.29** | 3.21 | 3.55 |
| 16k sequential | 12.78 | **4.29** | 4.68 | 4.68 |
| 1m repeat | 5.96 | **4.29** | 4.03 | 4.12 |
| 1m sequential | 16.43 | **4.32** | 9.97 | 10.72 |
| 20m repeat | 5.45 | **4.29** | 3.58 | 5.01 |
| 20m sequential | 18.30 | **4.56** | 7.85 | 7.72 |

(ns per iteration.) Instructions retired per iteration, the same loops minus a
zero-iteration run: repeat 116 -> 16, sequential 166-184 -> 24-25. The
wall-clock win is smaller than the instruction win because a 16-instruction
body is latency-bound on the element load, not instruction-bound.

No-regression cells (`benchmarks/json_performance/worker.ts`, five interleaved
rounds): `heterogeneous_1m parse` -2.2%, `records_array_1m parse` +0.2%,
`records_array_1m sparse` +0.2%, and `records_array_16k scan` **-30.1%** — the
counter-indexed loop inside that cell is the same clone, over an array the
preheader now materializes in bulk instead of leaving lazy for per-element
reads.
