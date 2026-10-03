**perf(arguments): an arguments object is born with its final shape (#10509)**

An escaping `arguments` object (dayjs's `cfg.args = arguments`) cost about
16,400 instructions per call against node's 179 (91x). Most of it was shape
work that does not vary per object:

- Marking the object as an exotic read receiver moved it onto a private
  shape lineage (a fresh semantic generation), so every call minted new
  ShapeIds from a counter that parks at the end of its 2^30 range. The mark
  already restamps the [[Prototype]] identity to `PROTO_ID_PER_OBJECT`, which
  no ordinary receiver's shape carries, so the private lineage added nothing.
  Arguments objects of one arity and callee kind now share one shape.
- The object was allocated with an empty shape, given its canonical keys
  (a second publish), then marked (a third). It is now allocated unshaped,
  marked first, and stamped once with the shape its arity was last born with
  (`try_birth_stamp_preinstalled_shape` re-checks every fact against the
  live descriptor; a miss publishes and remembers the id).
- A strict arguments object installed its restricted `callee` through the
  generic define-property path: a key search, a string, and a fresh accessor
  pair per call. The key is born in the canonical layout and pairs are
  immutable, so one rooted pair is now stored straight into its slot.

escape_arguments: 16,365 -> 7,799 instructions per call (91x -> 44x node).
What remains is mostly the field stores' GC layout notes and the caller's
key-add stores (`cfg.date = ...`, which escape_rest pays too: 4,573).

Tests: `test_gap_10509_arguments_shared_shape_sloppy.cts` (sloppy aliasing,
strict `callee`, shared-shape objects mutated independently, many objects
across collections) and two runtime unit tests that pin the shape count and
the shared pair.
