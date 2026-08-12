**fix(gc): root `Object.defineProperty`'s receiver, key and descriptor fields across its own allocating calls (#7963)**

A program that ran some allocating work and then installed properties with
`Object.defineProperty` faulted on a retired from-space address under the
quarantine (`PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1
PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`). This is the
window #6949's scope note names and defers, and the one #7949 deliberately left
open — it reproduces with a hand-written `defineProperty` loop, i.e. with
`Object.defineProperties` (the helper #7949 fixed) not on the path at all.

`js_object_define_property` resolved the receiver's `ObjectHeader` and coerced
the key to a `StringHeader` once, near the top, and then carried both — plus the
three NaN-boxed words `obj_value` / `descriptor_value` / `key_value` — as bare
Rust locals to the end of the function, past `define_array_property`,
`enforce_define_property_invariants`, `obj_value_has_own_key`,
`ensure_key_in_keys_array`, `clone_closure_rebind_this`,
`define_property_force_store_value`, and every `desc_has_field` /
`desc_read_field`. Those last two allocate a field-name string per probe and, on
a descriptor whose fields are accessors, run **user JS** mid-define. A raw Rust
local is neither a shadow slot nor a temp root nor reachable from any registered
scanner, so an evacuating minor could neither keep those objects alive nor
rewrite the local — and `scripts/gc_root_dominance_check.py` reads emitted LLVM
IR, so it is structurally blind to the class.

The stale receiver was the worse half: `obj as usize` is the OWNER KEY of the
per-property descriptor side tables, so a define that landed after a collection
filed its attributes and accessors under a dead address where the matching read
can never find them — a silent wrong answer rather than a crash.

Three sites:

* **`object_ops/define_property.rs`** — all five values are rooted in one scope,
  and an `across!` macro is now the only way to name any of them across a call:
  it runs the call first and rebinds all five from their roots afterwards, so a
  pre-collection address is never nameable. The descriptor's `get`/`set` field
  values, the existing accessor's closure bits (written back into the GC-scanned
  accessor table when the redefining descriptor omits a field), and the
  class-prototype mirror's method value are rooted for the same reason. The
  three per-arm `RuntimeHandleScope`s collapse into one — an inner scope dropped
  while an outer one is still taking handles truncates the outer container's
  newest entries (the hazard documented on `gc::RootedValues`).
* **`object_ops/descriptor_helpers.rs`** — `DescView`'s six field values were
  raw `JSValue`s read at decode time and handed back a dozen statements later;
  the stale word was then *stored into the receiver*. Each present field is now
  a `RuntimeHandle`, so `read` returns the post-collection address.
  `validate_nonconfigurable_redefine`'s per-field arm likewise roots the
  descriptor, the current value and the current accessor bits, and re-resolves
  `desc_ptr` *after* the allocation that precedes each read.
* **`object/reflect_support.rs`** — `obj_value_has_own_key`'s final keys-array
  walk held `keys` and `key_str` across `js_array_get`, which materializes a
  lazy array and therefore allocates. Both are rooted and re-read per iteration.

**Proof.** `crates/perry-runtime/src/gc/tests/rooted_define_property.rs`:
`define_property_lands_on_the_receiver_a_descriptor_getter_moved` drives the
real `#[no_mangle]` entry point with an accessor-backed descriptor whose getter
forces a copying minor, and asserts `copied_objects > 0`, that the **receiver's
and the key string's addresses changed**, that the property reads back the
getter's payload bytes, and that `get_property_attrs` finds the entry at the
**live** address. `desc_view_field_values_are_rooted` does the same for the
`DescView` fast path. `unrooted_receiver_copy_still_names_from_space` is the
sabotage arm: the identical address in a plain Rust `usize` keeps naming
from-space in the same cycle in which the rooted handle moves, which is what
makes the other two non-vacuous.

**Compiled probe.**
`test-files/test_gap_gc_define_property_descriptor_rooting.ts` — an allocating
`Object.groupBy` arm, a hand-written `Object.defineProperty` loop, and a loop
whose descriptor bag carries allocating accessor getters. Under the witness
configuration it exits **138** with `[gc-fromspace-protect] FAULT` (`obj_type=3`
= `GC_TYPE_STRING`, faulting at `user_ptr + 4`, which is
`StringHeader::byte_len` — i.e. the stale coerced key) on a pristine
`origin/main` build, and **0**, byte-exact against node 26.5.1, on this branch.

`scripts/raw_handle_debt.py` falls by 5 (`define_property.rs` 3 → 2,
`reflect_support.rs` 4 → 3); the recorded baseline is deliberately left
unchanged so parallel debt-paying PRs do not collide.
