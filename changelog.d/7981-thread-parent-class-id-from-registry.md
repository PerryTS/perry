### `perry/thread` no longer replays a plain object's ShapeId stamp as a class-parent edge

`ObjectHeader.parent_class_id` carries two different things: the parent class id
for a class instance, and — since #6759 C3c — the runtime **ShapeId stamp** for a
plain object (`class_id == 0`), written lazily by every by-name resolve path.

`thread.rs::serialize_object` copied the raw word, and the worker-side
deserializer hands it to `js_object_alloc_with_parent`, which does
`if parent_class_id != 0 { register_class(class_id, parent_class_id) }`. So any
object literal that had been read once and then crossed a `spawn` /
`parallelMap` boundary registered `class 0 → <a shape id>` in the process-global
class-parent registry (`PARENT_DENSE[0] = shape_id + 1`,
`CLASS_REGISTRY[0] = shape_id`) and bumped the store-plan epoch, once per
deserialized object. Every consumer checked guards `class_id == 0` off, so no
live victim was found — but it is registry pollution reachable from ordinary
user code.

The authoritative parent edge never lived in the header. Every parent-chain walk
reads `get_parent_class_id(class_id)`, and each edge is registered from a
compile-time constant: by `js_register_class_parent` in the module-init prelude
for codegen's inline `new C()` path (which writes the header word and
deliberately skips the per-alloc `register_class`), and by `register_class`
inside every runtime allocator that takes a `parent_class_id` argument. The
serializer now reads it from there.

This also removes the **last consumer of the header word as inheritance data** —
the blocking dependency for #6759 Phase C3's unification of class layouts and
plain-object shapes into one shape-id space, which is itself the prerequisite for
#7916's header shrink (`class_field_inline_guard` can only trade its
`keys_array`-identity compare for a one-word ShapeId compare once a class
instance has a shape word).

Three tests, each written to fail for a stated reason:

- the stamp is asserted present in the fixture *before* asserting it does not
  reach the wire, so the test cannot pass vacuously;
- a class instance's parent still round-trips *from the registry* with the header
  word deliberately overwritten by a stamp, so the fix is not "always send 0";
- `object/delete_rest.rs::shape_transition_tests_6759` pins both halves of the C3
  entry gate — a plain object's `delete` mints a genuinely different ShapeId,
  while a class instance's `delete` compacts its slots leaving `class_id` **and**
  `parent_class_id` untouched. The second goes red when a future rung gives class
  instances a stamp, which is the intended signal to switch the guard.

Refs #6759, #7916.
