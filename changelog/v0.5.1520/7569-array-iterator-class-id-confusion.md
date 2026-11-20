### Bug fixes

**`arr[Symbol.iterator]` read an array's `capacity` as a `class_id`, so an
array literal could resolve its iterator to an unrelated user class's `values`
method — self-recursively, and fatally (#7563).**

Reported as "a `class X extends Map` that overrides `values()` SIGSEGVs when
the override is iterated". `Map` turns out to be incidental, and so does the
iteration: the same crash reproduces with **no `Map` anywhere in the program**
and with **no `for-of` on the path**.

```ts
class Plain {
  values(): IterableIterator<number> {
    return [777][Symbol.iterator]();   // <-- SIGSEGV
  }
}
new Plain().values();
```

#### Root cause

`ObjectHeader` is `{ object_type: u32, class_id: u32, … }`; `ArrayHeader` is
`{ length: u32, capacity: u32 }`. The two `u32`s at offset 4 alias, so an array
pointer read as an `ObjectHeader` reports its **capacity** as a `class_id`.

`arr[Symbol.iterator]` resolves through `js_class_method_bind(arr, "values")`
(`symbol/get.rs`, the #321 arm that makes `typeof arr[Symbol.iterator] ===
"function"` hold). That builder's receiver→class step,
`class_id_from_method_receiver` in `object/native_module.rs`, read the field
with a bare `(*obj).class_id` — guarded only against closures and against the
handle band, never against the allocation's actual *type*. So whenever the class
whose id equalled the array's capacity happened to own a method named `values`,
`method_owner_class_id` found it and the canonical class method was returned as
the array's iterator.

The default array capacity is `MIN_ARRAY_CAPACITY`-clamped, and class ids are
handed out from 1 in declaration order, so the collision is not exotic — it is
the common case for small programs. When the colliding class was the *calling*
class, `values` re-entered `values` once per step until the stack guard page:

```
perry_method_repro_ts__MyMap__values
  → js_native_call_method_value → js_object_get_symbol_property   [the array read]
  → js_native_call_value → dispatch_bound_method → call_vtable_method
  → perry_method_repro_ts__MyMap__values                          [× ~26 000]
```

`EXC_BAD_ACCESS (code=2)` at `str xzr, [sp], #-0x50` — a stack-overflow guard
fault, not a stale or null pointer.

#### Fix

`class_id_from_method_receiver` now uses `js_object_get_class_id`, the guarded
accessor that already existed for exactly this read: it rejects the handle band,
the `std::alloc`'d `Map`/`Set`/`Regex` headers (which have no `GcHeader` to
probe), and any allocation whose `GcHeader.obj_type` is not `GC_TYPE_OBJECT`.
The bare read bypassed all three. The sibling symbol-method arm in
`object/native_call_method.rs` already routed through that accessor and was
never affected — verified, not assumed.

One line of behaviour change; the guard is deliberately not narrower than the
invariant it protects, so a genuine class instance still resolves to its id
(asserted in the same test).

#### Not #7561

The #7561 `for (… of m.values())` rewrite is **not** implicated, and neither is
any Map fast path. `rewrite_collection_view_for_of` declines a subclass receiver
exactly as its doc comment claims, and the crash needs neither a `for-of` nor a
`Map`: calling `m.values()` and discarding the result is enough, and a plain
class reproduces it. The offending line predates #7561 by hundreds of commits
(it traces back through #5631's file split to #4630), matching the issue's
report that it reproduces at `969b447cc`.

#### Also fixed by the same line

The related shapes in the issue's table now match node as well — a `values` /
`keys` / `entries` / `[Symbol.iterator]` override on a `Map`, `Set` or `Array`
subclass, an indirect subclass, and a class-expression subclass, with
`super.values()` from inside an override still reaching the native base.

Coverage: `test-files/test_gap_7563_array_iterator_class_id_confusion.ts`
(byte-compared against node; SIGSEGVs at the parent commit) and
`object::tests::array_receiver_is_never_read_as_a_class_id` (fails with
`Some(16)` — the array's capacity — before the fix).
