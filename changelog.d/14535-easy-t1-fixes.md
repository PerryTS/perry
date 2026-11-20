The `perry-runtime` test binary links on macOS again. `static_record_bounds`
declared an `extern_weak` static unconditionally while the `.weak` definition
that satisfies it is ELF-only, so Mach-O reported `_perry_fnrec_bounds` as a hard
undefined symbol and the test target could not be built; Apple now returns `None`
and the callers take the allocation-free scan they already use for unloadable
images.

A type-parameter argument satisfies every constraint again. `check_constraint`
compared the uninstantiated `Type::TypeVar` with the parameter's constraint, so
`types_satisfy` fell through to plain equality and nested generic calls warned
where tsc accepts them. A type parameter's bound is enforced where it was
instantiated.

`perry init` no longer writes `[build] opt_level`. No crate read that key — the
level comes from `PERRY_LL_OPT_LEVEL` — so every scaffolded project carried a
setting that did nothing.
