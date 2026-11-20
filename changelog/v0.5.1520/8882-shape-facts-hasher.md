Dropped SipHash from `ids_by_facts`, the last shape-table map still using it.

Profiling `claude -p` put `RandomState::hash_one` at 17 self-samples inside
`shapes::` alone (57 across the process) — pure hashing overhead on a lookup
that runs on every descriptor install and retire.

Its sibling maps already moved off SipHash (#8125). The standing comment on
this one argued only against `PtrHasher`, whose `write_*` methods OVERWRITE the
accumulator — right for a single-word key, wrong for this five-field one, which
would collapse to its last field and collide every descriptor sharing it. That
objection does not apply to `FastKeyHasher`: it implements only `write`, so the
derived `Hash`'s `write_u32` / `write_u64` calls all forward there and FOLD with
FNV-1a, reaching every field.

The key is internal shape state, never program input, so DoS-resistant hashing
buys nothing — the same rationale already applied to the descriptor side tables.

The new test pins the folding property directly: vary one field at a time and
require a distinct hash each time. Sabotage-checked against `PtrHasher`, where
it fails with "changing `keys` alone must change the hash".
