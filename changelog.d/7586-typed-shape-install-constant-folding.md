**The typed-shape install stops re-deriving compile-time mask facts on every
construction. `push_cls` 1.09x, `churn_alloc` 1.07x, `churn` 1.04x, at +0 bytes
(#7578).**

`gc::layout::typed_shape_layout_entry` was re-measured on the pinned quiet host
before any code was written, because three tickets in this campaign were worked
from a headline that had already collapsed. It had not: **22.7% of `push_cls`
self time**, the largest single symbol in object construction, with
`layout_forget_object` a further 4.1% — 26.7% between them for a steady-state
path whose entire content is two header bit writes.

### The ticket's stated hypothesis was wrong, and the disassembly says why

#7578 proposed that the cost is the FFI call and the thread-local resolutions it
performs — the shape #7566 won 1.81x on — and that the remedy is to emit the hit
path inline at the `new` site. The emitted prologue looked like it agreed:

```
sub  sp, sp, #0x150          ; a 336-byte frame
stp  x28, x27, [sp, #0xf0]   ; ...and six pairs, twelve callee-saved registers
stp  x26, x25, [sp, #0x100]  ;    spilled and reloaded on every construction
...
```

LLVM sizes a prologue for a function's worst path, and the worst path here — the
descriptor build, the `RefCell` borrow on `SHAPE_LAYOUTS`, the hash — runs once
per shape. **That hypothesis was tested and is false.** Outlining the install
behind `#[cold] #[inline(never)]` cut the frame to 80 bytes and the twelve
callee-saved spills to zero, and made the benchmark *slower*: `push_cls` 0.72 ->
0.75 s, `churn_alloc` 0.72 -> 0.79 s, reproduced across two runs. On this core
the spills are cheap dual-issued stores off the critical path; removing them
bought nothing, and keeping six arguments live to forward to the outlined call
cost a dozen register moves that were not there before. **Instruction count is
what this function is bound by, not frame size** — which is also why #7566's
inline-bump win does not generalise here.

### Where the 22.7% actually goes

Counted off the disassembly, roughly 30 of the ~70 instructions on the hit path
re-derive facts that are **compile-time constants of the class**, per call,
because the FFI boundary makes them opaque parameters:

- **12 instructions** normalising two `(pointer, length)` pairs into slices.
  `slice::from_raw_parts` needs a non-null aligned pointer, so a `null` mask
  became `NonNull::dangling()` through a `csel` chain — to build two slices the
  hit path then only ever compares as integers.
- **~11 instructions** of `words_intersect` setup, asking whether the raw-f64
  and pointer masks overlap. They are two immutable globals; the answer was
  fixed when the class was compiled.
- **~6 instructions** computing `gc_type_layout_slot_kind` a second time.
  `layout_header_for_user` already computed the kind and accepted three of them;
  the next line loaded through the 32-byte-strided type table again to narrow
  those three to one.

So the fix is not to move the call, it is to stop paying for the constants.

- The construction path now carries the raw `(pointer, word count)` pairs and
  materialises a slice only where one is actually indexed — the validating entry
  point's per-slot loop, and the install.
- The mask-disjointness check moves **below** the memo probe. A hit proves an
  install already ran it over the *same* globals and passed: a shape whose masks
  intersect is downgraded before it can reach `record`, so no intersecting tuple
  can be in the table to hit. It still runs ahead of every install, which is the
  only place its answer is used.
- The memo carries one bit, `dims` bit 62: whether the pointer mask is empty,
  which selects `GC_LAYOUT_POINTER_FREE` vs `GC_LAYOUT_SIDE_MASK`.
- One `gc_type_layout_slot_kind`, not two.

**Why replaying two mask predicates is not the thing the memo's soundness bar
forbids.** That bar is about the *object*: `field_count == slot_count` and the
per-slot validation stay per-instance, because an object's contents change under
the mutator. These two predicates read only the mask globals, which are
codegen-emitted `private unnamed_addr constant`s — in the read-only image, never
written, never freed. An entry matches on those globals' addresses and lengths,
so a matching address *is* a matching byte string for the life of the process,
and a pure function of those bytes has one answer forever. The residual failure
mode is still a miss, never a wrong hit.

| bench | main | this | ratio |
|---|--:|--:|--:|
| `push_cls` | 0.72 s | **0.66 s** | **1.091x** |
| `churn_alloc` | 0.72 s | **0.67 s** | **1.075x** |
| `churn` | 1.00 s | **0.96 s** | **1.042x** |
| `cycles` | 0.83 s | 0.83 s | 1.00x |
| `retain` | 2.89 s | 2.88 s | 1.00x |
| `tree` | 8.45-8.63 s | 8.47-8.55 s | ~1.00x |
| `deeplist` | 1.52 s | **1.53-1.54 s** | **0.987-0.993x** |

Best-of-7 wall clock on the pinned quiet host at load < 2, two independent runs
per arm, `main` re-measured between them (it reproduced to the millisecond on
all three of its runs), and the whole set re-confirmed after rebasing onto
`main`'s newer head. **`deeplist` pays 0.7-1.3%**, reproducibly across three
runs: its nodes have pointer fields, so it takes the validating entry point,
whose per-slot loop now builds its two slices inside the loop's own branch
instead of finding them hoisted.

The leaf profile moves the way the mechanism predicts:
`typed_shape_layout_entry` 22.7% + `layout_forget_object` 4.1% = **26.7% of
710 ms** becomes `init_typed_shape_layout` 12.6% +
`js_gc_declare_typed_shape_layout` 2.1% + `layout_forget_object` 5.6% = **20.4%
of 650 ms** — 190 ms down to 132 ms, against a 60 ms wall-clock improvement.

**Binary size: +0 bytes, and structurally so.** The diff touches two files, both
in `perry-runtime`; no codegen crate is modified, so emitted IR is unchanged by
construction. Measured either way, all seven benchmark binaries are byte-for-byte
the same size as `main`'s (12,222,200 each), and the symbol-carrying build is
320 bytes *smaller*.

### The codegen remedy the ticket proposed is unsound — do not revisit it

Worth recording, because it looks free. The classes that take the `declare` path
must have an **empty pointer mask** (`class_layout_declarable_at_allocation`), so
their declared state is `GC_LAYOUT_POINTER_FREE` — byte-identical to what the
allocator already writes. Since #7566 a `new` inside a loop writes its `GcHeader`
as a single i64 constant store, so OR-ing `GC_OBJ_TYPED_LAYOUT_INTACT` into that
constant would set the bit for **+0 instructions and +0 bytes**.

It would also be a use-after-free factory. Setting the bit without an installed
descriptor breaks the invariant "intact ⟹ some descriptor is reachable", and
`layout_note_slot` has a hole that only that invariant closes: on a contradicting
store to an intact-but-descriptor-less object it resolves a `None` verdict, falls
through to the ordinary pointer-mask bookkeeping, moves the object to
`SIDE_MASK` — and never clears the intact bit, because `layout_set_typed_unknown`
is reached only from the `Some(verdict)` arm. The object is then simultaneously
`SIDE_MASK` (the collector believes slot K holds a live pointer) and intact (the
codegen-inlined class-field guard in `expr/class_field_inline_guard.rs` believes
slot K is raw-f64). That guard consults no map by design, so it passes, and
`property_set.rs`'s raw-store fast path stores a double over the pointer **with
no write barrier and no layout note** — after which the next collection walks
slot K as a heap pointer. `layout_transfer` re-derives the bit correctly on
evacuation, but only for objects that are actually evacuated, so the window is
the object's lifetime.

### Testing

`cargo test -p perry-runtime` (1820) and `-p perry-codegen --lib` green;
`check_file_size.sh`, `addr_class_inventory.py`, `raw_handle_debt.py` (998,
unchanged) and `cargo fmt --all -- --check` clean; `gc_root_dominance_check.py`
in both gated modes with `--seeded-violations 40`, which this change cannot
affect since it emits no code.

Two new tests, both sabotage-verified rather than merely run:

- `the_pointer_mask_empty_bit_round_trips_per_entry` — the replayed bit must be
  per-entry and must be the recorded one. Dropping it from `pack_dims` turns it
  red, and so does making `hit` return a constant `Some(true)`, which also takes
  down `memo_installed_objects_survive_a_copying_minor_with_their_children` (the
  GC witness) and `a_memo_hit_produces_the_same_header_state_as_the_install`.
- `packed_dims_fields_do_not_overlap_the_empty_bit` — the word-count fields
  narrowed from 20 bits to 19 to make room for bit 62; widening one back turns
  it red. An overlap would make a wide-mask shape read back as `POINTER_FREE`,
  and the collector would skip payload slots holding live pointers.

The existing `a_contradicting_field_is_refused_even_with_the_memo_warm`
earned its keep: an earlier draft of this change probed the memo before the
per-slot validation, and that test caught it on the counter assertion its message
names. The probe now sits after validation, exactly where it was.
