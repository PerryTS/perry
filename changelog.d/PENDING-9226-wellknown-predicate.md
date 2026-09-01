**fix(hir): one source of truth for special-lowered well-known symbols**

`const E = class { [Symbol.toPrimitive](hint) { … } }` coerced through the
default `toString()` instead of the hook — `String(E)` returned `"WRONG"` and
`+E` returned `NaN`.

#9226 narrowed `generic_computed_member_key` by hand-copying a SUBSET of the
conditions in `lower_well_known_computed_method`: it listed `iterator`,
`hasInstance` and `toStringTag`, while the helper actually handles six shapes.
The four it omitted — `toPrimitive`, `asyncIterator`, `dispose`,
`asyncDispose` — stopped being exempted from the generic computed-member path,
so they lost the lifting/renaming the runtime resolves them by and were simply
not there at dispatch time.

The fix is structural rather than a one-symbol patch: both callers now ask
`is_special_lowered_well_known`, so the two lists cannot drift apart again.
Only `toPrimitive` had test coverage; the other three were silently broken.

Bisected on an isolated host with CLEAN builds at every point (an incremental
target dir carried stale artifacts across checkouts and produced false
verdicts — every result here is from `cargo clean` first):

| commit | `issue_9101_class_ref_coercion` |
|---|---|
| `9c8cdfc2e9` | passed |
| `015ec5fe1c` (#9300, parent) | passed |
| `8b2cfe6e7b` (#9226) | **FAILED** |
| `8b2cfe6e7b` + this fix | **passed** |

`015ec5fe1c` is #9226's direct parent, so the isolation is exact. Verified in
all three directions — the failing test passes, `issue_5128_user_symbol_iterator`
stays green (3 passed), and #9226's own
`test_gap_9226_class_prototype_own_keys.ts` still matches Node byte-for-byte,
so its own-keys win is preserved rather than traded away.
