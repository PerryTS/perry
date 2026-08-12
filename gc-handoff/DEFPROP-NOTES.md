# #7963 — `Object.defineProperty`'s own receiver / key / descriptor-field window

Working notes for the `gc/7963-define-property-rooting` branch. Written
incrementally; the PR body is the summary, this is the audit trail.

## The class

Raw NaN-boxed values and raw heap pointers held in ordinary Rust locals across
calls that can allocate. Neither shadow slots nor temp roots nor reachable from
any registered scanner, so an evacuating minor can neither keep them alive nor
rewrite them. `scripts/gc_root_dominance_check.py` reads emitted LLVM IR, so it
is structurally blind to the whole class.

#7949/#7962 closed the *container* shape (`Vec<f64>` accumulators). This is the
`obj` / `key_str` / `DescView` shape #6949's scope note names and defers:

> `js_object_define_property` also holds `obj` / `descriptor_value` and the six
> raw `JSValue`s inside `DescView` across its own later `js_string_from_bytes`
> calls, and `obj_value_has_own_key` holds `keys` / `key_str` across a
> `js_array_get` walk that can materialize a lazy array.

## The pristine fault, reproduced and localized

`test-files/test_gap_gc_define_property_descriptor_rooting.ts` under the witness
configuration, on a pristine `origin/main` release build
(`a769fafc6`, `PERRY_RUNTIME_DIR` pinned to that build's `.a` pair):

```
exit 138, stdout stopped after "objectGroupBy ok"

[gc-schedule] FAILURE (signal 10) under seed=1
[gc-schedule]   safepoints=135 scheduled_collections=135

[gc-fromspace-protect] FAULT: signal 10 at 0x28207c40e5c
  This address is RETIRED FROM-SPACE. ...
  block=0x28207c40000 +3676 retired_bytes=4104 retired_by_minor=#134
  last-known object: user_ptr=0x28207c40e58 obj_type=3 size=216
```

The instrument is live (134 retiring minors before the fault), and the fault is
precise in a way worth writing down:

* `obj_type=3` is `GC_TYPE_STRING`.
* the faulting address is `user_ptr + 4`, and `StringHeader::byte_len` is at
  **offset 4** (`crates/perry-runtime/src/string/mod.rs:308`; offset 0 is
  `utf16_len`).

So the faulting instruction is a `(*key_str).byte_len` read on a stale
`*const StringHeader` — i.e. the coerced key of `js_object_define_property`,
which is exactly the pointer the issue predicted. This matches the report in
#7963 (same `obj_type=3`, same `size=216`) on a different box/seed.

The program never reaches arm 2's output, so the failing arm is the
hand-written `Object.defineProperty` loop — with `Object.defineProperties`
(#7949's helper) not on the path at all.

## Sites fixed

### 1. `object/object_ops/define_property.rs` — the ordinary-object arm

`obj` (`*mut ObjectHeader`) and `key_str` (`*mut StringHeader`) were resolved
once near the top and then carried, raw, to the end of the function — through
`define_array_property`, `enforce_define_property_invariants`,
`obj_value_has_own_key`, `ensure_key_in_keys_array`,
`clone_closure_rebind_this`, `define_property_force_store_value`, and every
`desc_has_field` / `desc_read_field` (each allocates a field-name string, and on
an accessor-backed descriptor field runs USER JS). `obj_value`,
`descriptor_value` and `key_value` were rooted only across the initial
`js_string_coerce` and then read as plain locals for the rest of the body.

The receiver is the worse half: `obj as usize` is the OWNER KEY of the
per-property descriptor side tables (`set_property_attrs`,
`set_accessor_descriptor`, `accessor_descriptors`), so a stale receiver files
the attributes and accessors under a dead address where the matching read can
never find them — a silent wrong answer, not a crash.

Fixed by rooting all five and introducing an `across!` macro that is the only
way to name any of them across a call: it runs the call first and rebinds all
five from their roots afterwards, so a pre-collection address is never
nameable. No new bare `get_raw_*_ptr` sites — `RuntimeHandle::across_mut` is
what the `scripts/raw_handle_debt.py` ratchet asks for, and the file's count
went 3 → 2.

Also rooted inside that arm:

* the descriptor's `get` / `set` field values, which spanned
  `ensure_key_in_keys_array` and the first of two `clone_closure_rebind_this`
  calls;
* the existing accessor's `get` / `set` closure bits, which are written back
  into the (GC-scanned) accessor table when the redefining descriptor omits a
  field, and which spanned the same two allocating calls;
* the class-prototype mirror's method value, which spanned
  `descriptor_enumerable` (two more descriptor field reads).

The three inner `RuntimeHandleScope`s (closure arm, typed-array arm, ordinary
arm) were collapsed into ONE scope created before `try_decode_descriptor`. That
is deliberate: the scope has to outlive the `DescView` handles, and an inner
scope dropped while an outer one is still taking handles truncates the outer
container's newest entries (the hazard documented on `gc::RootedValues`).

### 2. `object/object_ops/descriptor_helpers.rs` — `DescView`

`DescView` held six raw `JSValue`s read at decode time and handed them back at a
dozen points spread over the rest of `js_object_define_property`. The stale word
was not merely read — it was **stored into the receiver**
(`define_property_force_store_value`) or into the accessor table. Each present
field is now a `RuntimeHandle`, so `read` returns the post-collection address;
absent fields hold no handle and read `undefined` as before. `DescView` gained a
`'scope` lifetime; `try_decode_descriptor` takes the scope.

`validate_nonconfigurable_redefine`'s per-field arm (`desc_view == None`) also
allocated a field-name string per probe while holding `desc_ptr`, the current
value being compared, and the current accessor's closure bits. All three are now
rooted and re-read; `desc_ptr` is re-resolved *after* the allocation that
precedes each read.

### 3. `object/reflect_support.rs` — `obj_value_has_own_key`

The final keys-array walk held `keys` and `key_str` across
`crate::array::js_array_get`, which materializes a lazy array and therefore can
allocate. Both are rooted and re-read per iteration. The
`string_coerce_is_inert` shortcut around the scope was dropped: the walk needs
the same scope whatever the key's shape, so skipping it bought nothing. File's
raw-handle count went 4 → 3.

## How the fix is proven

`crates/perry-runtime/src/gc/tests/rooted_define_property.rs`, three tests, all
under `CopyingNurseryTestGuard` + `suppress_automatic_triggers`:

1. `define_property_lands_on_the_receiver_a_descriptor_getter_moved` — the
   end-to-end proof, through the real `#[no_mangle]` entry point. The descriptor
   bag's `value` field is an ACCESSOR whose getter forces a copying minor (which
   is what pushes `try_decode_descriptor` onto the spec-general path, so
   `desc_read_field` runs user JS mid-define). It asserts, in order:
   `copied_objects > 0`; the **receiver's address changed**; the **key string's
   address changed**; then that the property reads back the getter's payload
   bytes; then that `get_property_attrs` finds the entry **at the live
   address**. The last assertion is the one that catches a stale receiver, since
   the attribute table is keyed by address.
2. `desc_view_field_values_are_rooted` — `try_decode_descriptor`'s fast path,
   the `DescView` half: decode, force a copying minor, assert the field's
   address changed and it still reads the original bytes.
3. `unrooted_receiver_copy_still_names_from_space` — the sabotage arm. The same
   address held in a plain Rust `usize` (exactly what pre-fix
   `js_object_define_property` carried) keeps naming its pre-collection value in
   the same cycle in which the rooted handle to the SAME object is rewritten.
   This is what makes (1) and (2) non-vacuous.

### Sabotage verification (fix committed first)

See "Sabotage run" below.

### Compiled probe

`test-files/test_gap_gc_define_property_descriptor_rooting.ts`, three arms:
an allocating `Object.groupBy` first arm (to retire from-space blocks), a
hand-written `Object.defineProperty` loop, and a loop whose descriptor bag
carries three allocating accessor getters.

## Not covered by a moving test

* The `obj_value_has_own_key` keys-walk fix (site 3) is by inspection: the
  allocation there is a lazy-array materialization, which the unit harness has
  no cheap way to force. Stated rather than glossed.
* An accessor-install test (a collection between the descriptor's `get` and
  `set` reads) was written and dropped: it kept faulting inside the harness's
  own setup rather than in the code under test, and a test that fights the
  harness is not evidence. The window it targeted is covered end-to-end by the
  compiled probe's third arm.
