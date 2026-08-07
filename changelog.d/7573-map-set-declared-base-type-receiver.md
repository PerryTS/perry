### Bug fixes

**A `class X extends Map | Set` instance held in a binding *annotated* with the
base type was dereferenced as a raw `MapHeader`/`SetHeader` — SIGBUS on the
first `.set()` (#7570).**

```ts
class MyMap<K, V> extends Map<K, V> {}

const m: Map<string, number> = new MyMap<string, number>();
m.set("a", 1);          // <-- SIGBUS, exit 138, before anything prints
console.log("size:", m.size);
```

Node prints `size: 1`. Annotating a binding with a base type is everyday
TypeScript, and a parameter (`function f(m: Map<string, number>)`) is the more
likely way to hit it in real code — NestJS's `ModulesContainer extends Map` is
exactly this shape.

#### Root cause

Perry models a Map/Set subclass instance as a plain `ObjectHeader` carrying the
real collection under a hidden field (`object/map_set_subclass.rs`). The two
headers overlay field-for-field:

| `MapHeader` field | offset | actually reads on an `ObjectHeader` |
|---|--:|---|
| `size: u32` | 0 | `object_type` (= 1) |
| `capacity: u32` | 4 | `class_id` |
| `entries: *mut f64` | 8 | `parent_class_id` ‖ `field_count` |

so `entries` is two `u32` class ids glued into a pointer, and the first store
through it faults (`map_set_string_key_value + 708`, `str x21, [x20], #0x8`).

The instance reaches those raw entry points because **"is a Map" was decided
from the declared TypeScript type**. `is_map_expr` / `is_set_expr`
(`perry-codegen/src/type_analysis/strings.rs:135`, `:13`) are satisfied by
`Type::Generic { base: "Map" }` with no subclass or runtime-brand check, and the
HIR fold that produces `Expr::MapSet` / `SetHas` / … keys on the same thing
(`perry-hir/src/lower/expr_call/local_array_methods.rs:915-1032`), as do the
for-of fast paths (`lower/stmt_loops.rs:1306`, `:1326`).

A declared type is a **hint, never a layout fact** — CLAUDE.md's *Known
Limitations* says annotations are erased and nothing validates them at runtime.
The unannotated form (`const m = new MyMap()`) always worked precisely because
it types as the subclass and dispatches through `subclass_backing_of`; the
annotation is what routes the value onto the raw lowering.

#### Fix

Resolve the receiver at the **raw runtime entry points** rather than tightening
one codegen predicate at a time. `map::clean_map_ptr` and `set::clean_set_ptr` —
the funnels 27 and 30 `js_map_*` / `js_set_*` entries already share — now
brand-check what they are handed:

* a genuine header (`GC_TYPE_MAP` / `GC_TYPE_SET`) passes straight through. This
  is the only case that costs anything: one `GcHeader.obj_type` load, of the
  8 bytes immediately preceding the header, plus a compare;
* a `class X extends Map | Set` instance is redirected onto its hidden backing
  (`redirect_collection_receiver`, `#[cold]`/`#[inline(never)]`);
* a plain object *merely annotated* `Map<K, V>` resolves to null, so every entry
  degrades through its existing null branch (`undefined` / `0` / `false`)
  instead of dereferencing a forged pointer;
* anything with no readable `GcHeader` (handle-band ids, tag remnants,
  non-pointer garbage) is passed through unchanged — exactly the pre-fix
  behaviour.

This is the fail-closed option: it covers every binding form and every future
caller. Chosen over "refuse the raw lowering when the static type is a
subclassable native base", which would have cost the fast path for every
`Map<K, V>`-annotated binding in every program, and over a codegen-emitted guard,
which would have had to be repeated at each of the ~20 lowering sites.

Three sites needed more than the funnel:

* `js_set_add` / `js_set_has` / `js_set_delete` / `js_set_clear` /
  `js_set_to_array` never called `clean_set_ptr` at all — they went straight to
  `find_value_index` on the raw pointer.
* `collection_iter_object::{map,set}_iter_obj_raw` **store** the pointer into the
  iterator object instead of using it immediately, so the redirect has to happen
  before capture (`resolve_map_receiver` / `resolve_set_receiver`).
* `Map.prototype.set` and `Set.prototype.add` return their **receiver**. For a
  subclass instance the receiver and the collection differ, so the write goes to
  the backing while the instance comes back — otherwise `m.set(k, v) === m` was
  false and chaining handed out the backing. The receiver is rooted across the
  store (`RuntimeHandle::across_mut`), because it is a movable `ObjectHeader` and
  the store allocates.

#### Validation

* `test-files/test_gap_7570_map_set_declared_base_type.ts` — byte-identical to
  `node --experimental-strip-types`, exit 0. Covers all five binding forms
  (`const`, parameter, class field, return type, `as` cast), the whole iteration
  surface (`for-of`, spread, `Array.from`, `forEach`, `.entries()/.keys()/
  .values()`), `size`/`get`/`has`/`delete`/`clear`, receiver identity, indirect
  subclasses, a subclass with its own constructor and fields, and non-subclass
  controls including the specialized numeric- and string-keyed entry points.
  With the runtime change reverted the same file exits 138 with no output.
* Four sabotage-shaped unit tests in `object/map_set_subclass.rs`: each first
  asserts the header byte the pre-fix code misread is still there
  (`object_type == 1` at `MapHeader.size`'s offset) and only then that the entry
  point returns the resolved answer, so a green run proves the redirect fired
  rather than that nothing threw. The genuine-Map test additionally asserts
  `redirect_collection_receiver` returns 0 for a real `MapHeader`, so the
  fast-path identity cannot have come from a redirect that happened to agree.
* Fast path unchanged: the change is runtime-only, and the emitted LLVM IR for a
  probe exercising all the affected forms is **byte-identical** before and after
  (`diff` on `--trace llvm` output, 8,910 lines, zero differences) — plain
  `new Map()` still lowers to `js_map_set_string_number` / `js_map_get_string_key`
  / `js_map_size` exactly as before.
* `cargo test -p perry-runtime`: 1816 passed, 0 failed.

#### Not fixed here

`class X extends Array` is the same hazard on a different family — an Array
subclass instance is also a plain `ObjectHeader` (`array/subclass.rs`), and
`is_array_expr` gates three tiers that do not brand-check: the bounded-index
element get/set, the inline `arr.length` load, and `lower_array_method`. The
general index-get tier already guards (`expr/index_get/guarded_array.rs` tests
`GC_TYPE_ARRAY` before the slot load). Filed separately.

`m instanceof MyMap` is false for a Map/Set subclass instance with **or
without** the annotation — a pre-existing, unrelated gap in the class-registry
parent edge, not touched by this change. Filed separately.

Every other native base in the sweep already re-validates at the runtime
boundary: Promise (`subclass_backing_promise` in `promise/checked_dispatch.rs`),
RegExp (`is_valid_regex_ptr`), Error (`object_type == OBJECT_TYPE_ERROR`), Date,
DataView / typed arrays / Buffer (`lookup_typed_array_kind`,
`is_registered_buffer`), and URLSearchParams (`resolve_search_params_receiver`).
