### `perf(runtime)`: let the GC header pick the side-registry probe on dynamic dispatch (#7850)

`object::native_call_method::gc_pointer_and_type_from_value` sits on the path of
**every dynamic method call** (`js_native_call_method` → `class_vtable_fast_guard`).
It ran four address-keyed side-registry probes — `set::is_registered_set`,
`map::is_registered_map`, `regex::is_regex_pointer`, `symbol::is_registered_symbol` —
purely to *exclude* object kinds, and only then read the `GcHeader` that already
records the kind three of them were looking for.

The symbol probe is the expensive one: a process-global `pthread_mutex` plus a SipHash
over a `HashSet<usize>`. It already had a `RegistryLatch` and the latch is correct — it
is just **armed by almost every realistic program**, because `well_known_symbol()` is
what materialises `Symbol.iterator` and that is what a `for…of` lowering reaches for. A
latch a program arms in its first loop is not protection; it only moves the cost behind
a branch that is always taken.

The header now selects the probe, and each implication is enforced by the probe itself,
so this is a re-ordering rather than a new assumption: `is_registered_set` ends in
`obj_type == GC_TYPE_SET`, `is_registered_map` in `GC_TYPE_MAP`, and `is_regex_pointer`
matches the magic of a `gc_malloc(_, GC_TYPE_OBJECT)` allocation (`js_regexp_new` is the
sole `REGEX_POINTERS` insert). A `GC_TYPE_OBJECT` receiver — the overwhelmingly common
case — now consults **one** registry instead of four, and never the symbol mutex.

#### The hole the header cannot cover, and why the fix is a content check

`symbol.rs` has five registration sites and they do **not** agree on storage: three of
them (`well_known_symbol`, `intl_legacy_constructed_symbol`, `js_symbol_for`) are
`Box::into_raw`, i.e. process-lifetime allocations with **no `GcHeader` at all**, so
`ptr - 8` is foreign allocator bytes that can read as any `obj_type`. Trusting the header
for those is exactly the #7846 shape — a proof that is true at one site and assumed
everywhere.

The first attempt screened them by *address*: a monotone `(lo, hi)` window over the
leaked-symbol addresses, `false` exact, two atomic loads. **Its own invariant test
refuted it** — in a full `cargo test` run the window read
`0x56bbcbd0680..=0x5b0100007673` and 64/64 freshly allocated GC objects fell inside it.
One outlier `Box` widens an address range to span the arena, and the fast path silently
stops firing: still sound, worth nothing. An address range over allocator-chosen
addresses is not a screen.

What every symbol *does* have, whatever its storage, is `SYMBOL_MAGIC` in its own first
four bytes — `alloc_symbol` and all three `Box` sites set it, and the field is at offset
0 precisely so cheap discrimination is possible. `symbol::may_be_symbol_header(ptr)` is
one 4-byte load of the object the caller is already about to inspect. `false` is exact
(no symbol reads `false`); a false `true` merely pays the old probe and gets the old
answer. It cannot be defeated by allocator placement, and it covers GC-heap and leaked
symbols with a single test.

#### Tests

* `probe_dispatch_tests::plain_object_dispatch_probes_no_side_registry` asserts the
  saving rather than assuming it: with the symbol latch **armed**, a plain-object
  dispatch must not move the symbol / map / set probe counters. Delete the `obj_type`
  dispatch and it goes red. (New `symbol::TEST_SYMBOL_REGISTRY_PROBES`, the same
  `#[cfg(test)]` idiom `map.rs`, `set.rs` and `arguments.rs` already use.)
* `header_directed_dispatch_needs_the_symbol_magic_screen` is a **sabotage** test: with
  the screen defeated, the dispatch must fall back into `is_registered_symbol` — and
  still give the same answer. A future edit that drops the screen cannot leave the suite
  quietly green.
* `the_magic_screen_covers_every_symbol_and_no_ordinary_object` pins both halves:
  soundness (every leaked *and* `gc_malloc`'d symbol carries the magic) and the
  performance invariant (0/64 fresh GC objects may read as the magic). This is the
  assertion that refuted the address-window design.
* `exotic_receivers_are_still_excluded` / `regexp_receiver_is_still_excluded` — the
  answer is unchanged for Set / Map / RegExp / fresh `Symbol()` / leaked symbol,
  including one created after the idle fast path already ran (#7474 shape).

#### Measured result: NULL on the current corpus, and why

Quiet M1 mini, best-of-7, exit-checked, one batched lock window: **21 programs, all
within ±0.7%** — the noise floor. Nothing here is a win and nothing is a regression, and
that includes two shapes written specifically for this change (`dyncall`, a base-typed
polymorphic tree-walk; `dynmix`, a mixed object/array/`Map` receiver loop; both with a
`for…of` so the symbol latch is armed the way a real program arms it).

The reason is #7852: it removed `pipeline`'s generic-specialization miss, and the
dynamic-dispatch load the probe was riding went with it (`pipeline` 0.483 s → 0.274 s).
#7850's own sizing caveat predicted exactly this. Symbolicated `sample` of
`bench/pipeline_big.ts` confirms it — **zero** samples reach
`gc_pointer_and_type_from_value` on either arm, while all 43 (baseline) / 40 (this
change) `is_registered_symbol_slow` samples come from `js_object_get_field_ic_miss →
get_field_by_name_tail`. The family did not go away; it moved to the property-get
IC-miss path (#7867).

This lands for the structural property and the assertion that locks it in, not for a
speedup: a plain-object dispatch now touches no side registry, and the probe counter
turns that from a hope into a test the next megamorphic program cannot quietly undo.

#### Refuted while scoping — #7850 named three sightings, two were already closed

* `visit_object_static_prototype_slot_mut`'s mutex + SipHash **per traced object** was
  fixed by #7859; `prototype_chain.rs:390` already opens with the
  `OBJECT_PROTOTYPES_NONEMPTY` gate and carries the `retain.ts` comment the issue quotes.
* `interp`'s `is_registered_set` / `is_registered_map` / `is_arguments_object` are
  already latched by #7469 and #7854; `PROFILE-interp-round3.md`'s shares predate both.

Two follow-ups filed with the profile evidence: **#7865**
(`js_dyn_index_get`/`js_dyn_index_set` probe the Set and Map registries on every dynamic
index access) and **#7867** (`get_field_by_name_tail` probes four registries before
reading the `GcHeader` it then switches on — where the family lives now).
