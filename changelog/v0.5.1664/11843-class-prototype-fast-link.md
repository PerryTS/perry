**runtime: a class finds its prototype through its own function object, and a declared prototype is born in its final shape (Refs #10502)**

`class_decl_prototype_object` (the "class -> its prototype" question asked by
`instanceof`, `getPrototypeOf`, `super`, method sites, holder reads, ...) went
through a thread-local RwLock and a hash map, with a reverse index beside it.
`C.prototype` is an own, non-writable, non-configurable property of the class
constructor, so the link is now a fact of the class object itself: the class
function object (one per agent per class, pinned) keeps its prototype in a
capture slot next to the class id. The forward read is the class-value table's
indexed loads plus one slot load; the reverse question (is this object a
declared class's prototype, asked of arbitrary objects on every
`defineProperty`/descriptor read) reads the object's class id from its header
and compares one link. The `DeclPrototypeTable` (forward + reverse maps) and its
GC scanners are deleted; the link is a root slot of the class-value scan.

A declared prototype whose members the class registry fully describes (methods
with entries, no accessors, no `C.prototype.m = f` registered before it existed)
is built as one allocation, its slot stores and one stamp of the final shape
(keys in ClassBody order with their attributes, the parent link, a ConstFn lane
per method), instead of a key-by-key define, an attribute claim per key, a relink
and a lane relearn. Other prototypes take the unchanged general path. The parent
prototype is resolved before the object exists, so an invalid `extends` value
throws before anything is linked.

- Startup that touches every prototype (300 classes x 8 methods): -32%
  instructions, -2.2 MB RSS.
- tsc -0.6% (RSS -11 MB), Zod -0.7%, commander -0.5%, qs -0.08% instructions.
