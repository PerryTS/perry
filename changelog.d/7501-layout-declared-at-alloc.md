**GC element layout is now declared once at allocation for proven pointer arrays,
instead of maintained per push (#7469 mutator-cost campaign).**

An `out = []` + `out.push({ … })` loop paid two runtime calls on every iteration
of its inline store: `js_gc_note_slot_layout`, which reaches into the
`LAYOUT_SLOT_MASKS` thread-local hashmap to set one bit of a per-array pointer
bitmap, and `js_array_note_numeric_write`, which clears a raw-f64 flag that is
already clear. The mask entry the first push creates also makes
`LAYOUT_SLOT_MASKS` non-empty for the rest of the process, so the `is_empty()`
fast-out in `layout_forget_object` — probed on **every** object allocation —
starts hashing instead.

When codegen can see the whole life of an array local (a single `[]` binding,
every store a push of a fresh allocation, no rebind, no keyed store, no closure
capture, no in-place mutator, and at least one such push in the same region), it
now emits one `js_array_declare_all_pointer_elements` at the allocation site and
drops both per-push calls. New collector: `perry-codegen/src/collectors/
all_pointer_arrays.rs`.

**Soundness does not rest on that static proof.** Every elided store is fronted
by a test of the live GC header, folded into the frozen/sealed integrity test the
inline push already performed — the same `_reserved` load, the same one `and` +
one `icmp`, no added instruction: `(_reserved & 0xF487) == 0xA000`. That demands
the four integrity bits clear (as before), **both** raw-f64 layout bits clear
(which is what makes eliding the numeric-write note sound — there is nothing left
for it to clear), and `GC_LAYOUT_SIDE_MASK | GC_LAYOUT_ALL_POINTERS` set (which is
what makes eliding the layout note sound — in that state the collector visits
every slot in `0..length`, so the slot being written is scanned whether or not a
mask bit was recorded). A push that fails the test falls through to
`js_array_push_f64`, which notes the slot exactly as it always did.

Testing the live header rather than trusting the declaration is load-bearing,
because the runtime revokes it: `js_array_is_numeric_f64_layout` on a still-empty
declared array verifies vacuously and re-publishes it as RawF64 + `POINTER_FREE`,
and `rebuild_array_layout` (sort/splice) installs a precise mask. Both are now
regression tests.

`layout_note_slot` gained one refinement: a pointer **append** at an array's
append position (`slot_index == length` — every append protocol in the tree
records the slot before bumping `length`) preserves an all-pointer declaration
instead of downgrading it. Without that the first growth through
`js_array_push_f64` demoted the array and every later push fell off the declared
path permanently.

`js_string_addref_if_heap_string` is now gated independently of the layout note
at array pushes (`store_needs_string_addref`). The declaration predicate admits
`Expr::New` — HIR rewrites closed-shape object literals to
`New { class_name: "__AnonShape_<hash>" }` before codegen runs, so the loop this
targets *is* the `new` form — and a `new` can be re-pointed by a constructor
return override, so it must never gate the string demote.

Witness coverage in `perry-runtime/src/gc/tests/copying/all_pointer_elements_7469.rs`:
an array filled through the elided sequence survives a copying minor with every
element relocated and every slot rewritten, and a permanent sabotage arm asserts
the *undeclared* array enumerates zero child slots — so a green positive test
means the declaration was load-bearing, not that nothing was tried.

Known non-firing shapes: both halves of a deforested producer/consumer pair (the
producer's `const out = []` becomes a `__deforest_out` parameter; the consumer's
literal has no push in its own region).
