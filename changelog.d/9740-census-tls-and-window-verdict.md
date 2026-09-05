**Convert `gc/census.rs` to hot TLS, and give the root-holder inventory a
verdict that fits its pass-1 snapshot.** `census.rs` was the last file keeping
`tls-budget` / `self-test-checkers` red on `main`: it declared two shipping
`thread_local!` blocks that `check_thread_locals.py` rejects, and every open PR
inherited the red X.

Converting them made `gc_runtime_root_holders.py` see four declarations for the
first time — it enumerates `perry_thread_local!`, so a raw block escapes both
gates, which is backwards: the declarations that skipped the convention are the
ones whose GC contract nobody audited. `ARMED`, `SEQ` and `LABEL` are a flag, a
counter and a `&'static str`, classified `not_a_gc_pointer`; the file's
`#[cfg(test)]` block is folded into the macro too (`test_only`), so no
declaration in `census.rs` is left outside the inventory.

`PASS1_MARKED` is the one #9740 was filed for. It holds real GC header
addresses, so `not_a_gc_pointer` — defined as an id, a counter, a code address,
.rodata or Rust-owned state — would have been a false statement about it, and
`covered_elsewhere` / `open_gap` / `unverified` fit no better. It is correct
because it is *untraced*: written at the end of mark propagation and consumed at
sweep entry of the same synchronous full cycle, under call-site guards that are
the identical predicate, in a window with no evacuation and no mutator resume,
and used only as `binary_search` keys. The new verdict
`untraced_in_nonmoving_window` says exactly that, and — because the window is
the whole safety argument — an entry must name the two functions that bound it
in `window_opens` / `window_closes`. The gate checks against the holder's own
source that both still exist and both still name the holder, so renaming a
boundary, deleting one, or moving the write or the take out of it fails the
gate. `--self-test` drives all three rejections.
