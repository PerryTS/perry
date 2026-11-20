Maps keyed by program input (static member names, native export names, JSON
property bytes, unwind sites) now hash with one per-process random key block
that mixes in no address, instead of `ahash::RandomState::new()`, which folded
the addresses of a static and of the stack into every seed. Flood resistance is
unchanged, since the keys still come from the OS once per process; probe order
and key-compare work no longer depend on link layout or the environment. The
timer handle table, keyed by our own monotonic ids, no longer seeds at all.
Wide JSON objects share the process key instead of drawing one per object.
The measurement-only `fixed-hash-seeds` feature (its marker and the flag in
`scripts/a1/build-arm.sh` / `irdiff.sh`) is removed: the default hasher state is
now layout-independent on its own.
