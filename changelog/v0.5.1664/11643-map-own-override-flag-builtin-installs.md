perf(runtime): populating `globalThis` no longer puts every `Map`/`Set`/`Date` builtin call on the slow side of the own-override guard (#10697).

The #10943 guard in front of a proven `Map`/`Set`/`Date` builtin call answers
"no own override" from one load of `PERRY_OWN_NAMED_PROP_INSTALLED`, and asks
the authoritative `hasOwn` predicate only once any non-ordinary cell has taken
a named property. The runtime's own lazy `globalThis` population set that flag:
it installs statics on constructor intrinsics such as `%TypedArray%` (closure
cells), `constructor` on `Array.prototype` (an array cell) and aliases such as
`Number.parseFloat`, and each of those stores passes the exotic-store gauntlet
that arms it. Any program that referenced `globalThis` or a lazily installed
global then paid about 600 instructions of `js_receiver_may_own_named_method`
→ `js_object_has_own` (with a key-string coercion) on every `m.get(k)` and
`m.set(k, v)`. That is the "count by category" shape #10697 measured at 4.2x
node.

- The runtime's own builtin definitions (`define_builtin_data_property` and all
  of `populate_global_this_builtins`) run inside `as_builtin_definition`. Inside
  it, an install arms the exported flag only when its owner is a Map, Set or
  Date cell, or its header cannot be read. Those are the only receivers whose
  guard answer comes from the flag: an array answers from its own header and
  named-property storage, and a declared-`Map` receiver of any other kind is
  brand-checked into generic dispatch. User installs arm exactly as before.
- The universal dispatcher's `own_user_method_value` now reads a separate
  internal flag that every install still arms, builtin or not, so its answers
  are unchanged.
- Small-map lookups (≤ 8 entries) answer a bit-identical key of any type from
  the inlined hot lane. A Map never holds two SameValueZero-equal keys and
  stored keys are normalized, so an identity hit is the match. Before, only a
  plain-number key could use that lane, and every string lookup paid the
  out-of-line call to `find_key_index_cold` first.

Measured (`callgrind`, instructions per iteration fitted over 20k→120k, x86-64,
`PERRY_NO_AUTO_OPTIMIZE=1`, program references `globalThis`): four constant
string keys, get-or-default then set, 1,866 → 373; a plain `m.get(CATS[i & 3])`
947 → 206, of which 111 is the array read itself. Output identical to Node.

Tests: `a_builtin_install_on_an_intrinsic_does_not_arm_the_guard` (272 arms
from one intrinsic install before; 0 now, while installs onto a Map, builtin
or user, still arm), `a_small_map_answers_an_identical_key_without_the_cold_path`
(8 of 8 identical lookups went cold before; 0 now, and content matches still
go cold), both sabotage-checked, and
`test_gap_10697_own_override_after_global_population` (own overrides on
Map/Set/Date/Array still win after population, and the populated builtins still
work). No version bump.
