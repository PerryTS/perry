**Fixed: JS values retained in ordinary Rust containers across allocating calls (#7949).**

`Object.groupBy` / `Map.groupBy` / `Object.defineProperties` — and, found by the
same sweep, the Proxy own-key builders — accumulated raw NaN-boxed JS values
into plain `Vec`s and then kept filling or walking them across calls that can
allocate: a user callback, a `[[Get]]` accessor, a key coercion, a result-array
build. A `Vec` on the Rust heap is neither a shadow slot nor a temp root nor
reachable from any registered scanner, so an evacuating collection could
neither keep those objects alive nor rewrite their addresses.
`scripts/gc_root_dominance_check.py` cannot see this class at all — it reads
emitted LLVM IR, and the container is Rust-side.

Root causes, per site:

- `object/groupby.rs` — `group_by_collect` returned `Vec<(f64, f64)>` filled
  across `js_closure_call2`. Three further holes in the same family: the
  materialized input array and the closure were hoisted out of the loop and
  re-dereferenced across the callback; `Object.groupBy`'s result object had **no
  root at all** (it is reachable from nothing else until it is returned) while
  spanning one array build and one key-string intern per group; and
  `group_by_make_array` returned a fresh array across
  `rebuild_array_layout_from_slots`. Symbol keys were also coalesced through a
  `HashMap` keyed on the symbol's *address*, so a symbol that moved mid-loop
  started a duplicate group — now keyed on `SymbolHeader::id`, which an
  evacuation copies verbatim (the #7246 argument).
- `object/object_ops/define_properties.rs` — `keys: Vec<f64>` was walked across
  `js_string_coerce`, a user getter on the properties bag
  (`js_dynamic_object_get_property`) and `js_object_define_property`, with the
  receiver, the bag, the own-names array and the coerced key string all bare
  locals in the same window.
- `proxy/own_keys.rs` — `alloc_key_array` grew the result array between key
  pushes, and `proxy_enum_own_keys` held the trap's key list across the proxy's
  `getOwnPropertyDescriptor` trap.

New `gc::RootedValues` (`gc/roots/rooted_values.rs`) is the reusable answer: a
growable list whose elements are `RuntimeHandle`s, so the registered
runtime-handle mutable-root scanner marks them and rewrites their slots. It
adds no root holder of its own and no `get_raw_*_ptr` sites.

Proof, not absence of a crash. `gc/tests/rooted_container_values.rs` runs every
assertion under a forced copying minor and gates on `copied_objects > 0`, on the
element addresses having *changed*, and on the post-collection pointer still
reading the original bytes; `plain_vec_of_values_is_not_a_root` is the sabotage
arm that shows the instrument distinguishes rooted from unrooted. Reverting only
`object/groupby.rs` to `main` makes the two end-to-end tests abort with
`TypeError: value is not a function`. The two new gap probes
(`test_gap_gc_container_value_rooting.ts`,
`test_gap_gc_define_properties_key_rooting.ts`) each exit 138 with a
`[gc-fromspace-protect] FAULT` on a pristine `main` build under
`PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_PROTECT_FROMSPACE=1
PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`, and are byte-exact against node 26.5.1
with the fix.
