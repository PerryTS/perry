Restored the literal `if descriptor.old_carrier || descriptor.cache_carrier` two-armed expression in `scan_shape_table_rekey_mut`.

#8899 lifted that condition into a `let is_carrier = …` binding for its memo key. The change was semantically inert, but `scripts/shape_descriptor_census.py` deliberately pins the *whole* two-armed expression — so that a sabotage which widens the gate or swaps the arms has to be red — and the refactor stopped matching that pin. `lint` has been red on `main` since #8899 landed.

It also silently disarmed the census's own self-test: that test sabotages this exact literal via `str.replace(old, new, 1)`, which does nothing when the string is absent, so the "un-gated into an unconditional table root" case was being replaced into nothing and proving nothing.

The condition is now written out at the decision site, with a comment saying why it must stay literal; `is_carrier` remains and still keys the memo.
