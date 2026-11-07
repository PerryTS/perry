A lazy `JSON.parse` array (an array root of 1 KiB to 16 MiB) now materializes
IN PLACE: the lazy value is born with an ordinary array's header and one
element slot per element, and materialization fills the slots that are still
holes and turns the same object into the ordinary array (`obj_type` flips from
the lazy kind to Array; the tape is released). One object, one address, so
identity (`===`, `Map`/`Set`/`WeakMap` keys), every compiled binding and every
element a caller already read stay valid across materialization, and a binding
that saw the lazy value takes the dense fast paths afterwards. The separate
materialized array, its sparse cache and bitmap side blocks, and the
redirect-to-materialized branches in the readers (stringify, clone, filter,
the record, binding repair) are gone. The flip keeps the header facts the
program already set on the lazy value: integrity level, the custom-prototype
bit and the copy age, and states the whole array as its dense prefix
(`ek_prefix`): a JSON value is never a hole.

The whole-array re-parse fills the slots as a bulk fill: one word copy (a
plain `memcpy` when nothing was cached yet, holes only otherwise), then one
layout rebuild with the range barrier for an old owner instead of a barriered
per-element store. A verbatim copy of a parser array whose builder proved
every slot a pointer installs that layout directly.

`for..of` over a lazy array no longer falls back to the iterator protocol on
every pass: the counted array record admits it without forcing it, element
reads go through the lazy array's own read policy (subtrees on demand; a
sequential run of max(64, n/64) reads flips to one whole re-parse), and the
same record continues on the inline array path once that flip happened. A
loop that breaks early keeps the laziness the tape paid for. Binding repair
(`js_array_refresh_local_head`, the shared read cold arm) never forces a lazy
array. `length` of a lazy array read through the generic path goes straight
to the array length.

`Array.from(array)` and `[...array]` of an ordinary array convert holes to
`undefined` only past the source's dense prefix: a hole-free array (a parsed
array, one built by `push`) copies like `js_array_clone`, and the copy states
itself dense.

A stale tape reader (`TapeSource::Lazy`) of a flipped value reads an empty
tape: `tape_slice`/`blob_bytes` test the object's kind, since the lazy fields
are hole-filled array reserve after the flip.

`delete parsed[i]` on an unmaterialized lazy array no longer panics: the
delete forces the array in place and leaves a hole, like any ordinary array.
