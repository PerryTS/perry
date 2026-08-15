### perf(object): remove the derivable `object_type` and `field_count` header words — 56 B → 48 B

`ObjectHeader` is now `{class_id @0, parent_class_id @4, keys_array @8, meta @16}`
— **24 bytes on LP64, 16 on ILP32**, down from 32/24. A two-field object literal
costs **48 bytes instead of 56** (`GcHeader 8 + header 24 + 2 slots`), and the
eight-slot case 96 instead of 104. Measured with `rustc -O` on the exact
`#[repr(C)]` shapes: removing **either** word alone saves **zero** — the struct
re-pads — so the two had to go together. This is half of #8047's prize and needs
none of its GC descriptor-rooting work (#8112).

Both words were derivable from facts the object already carries:

* the receiver **kind** — ordinary / class / native error — from
  `GcHeader.obj_type` plus the immutable ShapeId descriptor's `object_kind`;
* the **live inline-slot bound** from that descriptor's `live_inline_slot_count`.

#### The offset-0 type confusion this had to disarm

`ObjectHeader::object_type` was prefix-punned against `error::ErrorHeader`'s
first word, and **nine** sites read raw offset 0 to decide Error-vs-ordinary —
two more than previously catalogued (`promise/rejection.rs:181` and `:464`).
Deleting the word makes offset 0 `class_id`, and `OBJECT_TYPE_ERROR` is **2**
while class ids are handed out from 1, densely, in source-declaration order
(`run_pipeline.rs`: `let mut next_class_id = 1`). Left alone, those reads would
have reclassified **every instance of the second class a program declares** as an
`ErrorHeader` and served `message`/`name`/`stack`/`errors` out of its field
slots — a silent wrong answer of exactly the #8100 shape. Plain object literals
are `class_id == 0`, so the `OBJECT_TYPE_REGULAR` arms would have inverted in
both directions at once.

All nine now go through `error::ptr_is_native_error()` (`GcHeader.obj_type ==
GC_TYPE_ERROR`, the only kind `alloc_error` uses). Five sabotage-shaped
acceptance tests in `object/tests.rs` pin it: each first asserts the confusable
value really is sitting at offset 0, then asserts the answer. Reverting the
discriminator to the raw read turns three of them red.

`proxy.rs`'s #6595 store-plan gate moves to `object_is_regular()`. #8047's census
warned that this substitution re-opens #6595 — that warning was **stale**: #8086
rewrote `object_is_regular` to mean exactly `descriptor.object_kind == Ordinary`,
so it is still false for a heap class object. A test pins that too.

#### Mint-then-stamp

With `field_count` gone, the ShapeId descriptor is the **only** record of a live
object's slot bound, so a stamp-cleared window is a window in which the collector
traces zero payload slots — a fresh #7154/#7164. Every clear-then-remint sequence
is restructured: `shapes::publish_object_shape_from` mints the successor while
the predecessor stamp is still installed, and the single `parent_class_id` store
(which cannot allocate, hence cannot collect) is the publication point.
`set_object_keys_array`, `set_object_live_slot_count` and
`js_object_delete_field` no longer clear; `shapes::clear_object_shape_stamp` is
now `#[cfg(test)]`, surviving only so tests can manufacture the unstamped state.

`typed_feedback::object_shape`'s defensive self-heal is **deleted**. It called
`synchronize_object_shape_descriptor`, which derived the bound from the header
word; without that word it would publish `live = 0` for an unstamped receiver —
a read-only observation path silently truncating the object's traced and writable
payload. It misses closed instead. #6804's "no pre/post-stamp token split"
property survives by the stronger route: every allocator birth-publishes, so the
population needing a heal is empty.

#### Codegen

`object_header_size_bytes` 32 → 24 (LP64) and 24 → 16 (ILP32). The inline-`new`
path's two packed header stores collapse to one (`class_id ‖ ShapeId`). Eleven
hard-coded IR offsets renumber `class_id @+4` → `@+0` and ShapeId `@+8` → `@+4`;
GcHeader-relative offsets (`-8`/`-7`/`-6`) are untouched. Emitted IR never read
`field_count` — #8067 moved the PIC hit path onto an exact ShapeId match — so
codegen only ever wrote it.

The two `object_header_size_bytes(..) / 8` word-index sites (`expr/proxy_reflect.rs`,
`stmt/loops.rs`) become byte geps. Both quotients are exact today (24/8, 16/8),
but #8047's ILP32 header is 12 bytes and `12 / 8 == 1` truncates silently; a new
`object_header_size_is_a_whole_number_of_heap_words` test pins the divisibility
rather than the quotient.

Four codegen doc comments claiming "24 on 64-bit, 20 on ILP32" — wrong since
`meta` landed in #6759 — are corrected, along with `docs/src/platforms/watchos.md`
(the only user-facing statement of the pair), `docs/object-write-matrix.md`, and
`TYPE_LOWERING.md`.

#### Two gates that could not have caught this

**`perry-ffi`'s ABI mirror had never executed.** `object_header_matches_runtime`
is `#[cfg(all(test, feature = "runtime-link"))]`, `runtime-link` was enabled
nowhere in `.github/`, and `cargo-test` is a per-package loop with default
features — so the module never compiled. Field *deletion* still went red (an
`offset_of!` on a missing field stops compiling), but a **size or padding
divergence was invisible**, which is precisely this change's failure mode.
`cargo-test` now runs `cargo test -p perry-ffi --features runtime-link --lib`
unconditionally. It earned its keep on the first run: it caught a real bug in
this change — the parity `debug_assert` inside the new mint-then-stamp
publication compared the freshly stamped descriptor against a header keys word
the new ordering has not written yet.

**`perry-ffi` is published to crates.io.** This is a **breaking ABI change** for
out-of-tree wrappers: one compiled against the old mirror and linked against the
new runtime reads `class_id` out of the deleted `object_type` slot with no
compile error. That cannot be guarded retroactively — the old mirror references
no version symbol, so there is nothing the runtime can withhold. Recorded as a
deliberate break, with a tripwire introduced for the *next* one:
`perry_ffi::OBJECT_HEADER_ABI_REVISION` (= 2) paired with the runtime's
`extern "C" perry_object_header_abi_revision()`, asserted equal by the
now-running mirror test.

#### Gate updates

`scripts/shape_descriptor_census.py` narrows to `keys_array` (the last mirror)
and gains three rules the deletion needs: the exact `ObjectHeader` field list, so
re-adding a word is red rather than merely un-baselined; a ban on any publication
path clearing the stamp, plus a check that `clear_object_shape_stamp` stays
`#[cfg(test)]`; and a fixed emitted-guard offset rule. That last one was
**vacuous** — it matched only `add(..., "N")` while all four functions it names
emit `gep(I8, &p, &[(I64, "N")])` — so it now matches both spellings and requires
each guard to be shown reading the ShapeId at all. Three new sabotage self-tests
cover the new rules.

#### Also

* `perry-ui-android/src/json.rs` deleted — 606 lines, every function private with
  no callers, its own trailing comment saying `js_json_*` now lives in
  `perry-runtime/json.rs`. It read `field_count` in three places and is invisible
  to CI three ways (`#![cfg(target_os = "android")]`, outside the host-compatible
  workspace scope, and the only Android job is `continue-on-error`).
* `NullObjectBytes` gains the `meta` word it has been missing since #6759 — a
  `(*obj).meta` read on the unresolved-namespace stub was running 8 bytes past
  the end of the static.
* `object/mod.rs` reached the 2000-line cap, so `live_slots.rs` (the bound plus
  the ABI revision) and `null_stub.rs` split out.
