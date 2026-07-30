`benchmarks/run_public_baseline.sh` can now regenerate the published artifact from
a clean checkout. It built only `-p perry-runtime -p perry-stdlib -p perry`, but
those crates are `crate-type = ["rlib"]` — `libperry_runtime.a` and
`libperry_stdlib.a` come from the separate `perry-runtime-static` /
`perry-stdlib-static` wrappers. Without them `target/release/` held the `perry`
binary and no archives, so every `perry compile` in the measurement legs died with
"Could not find libperry_runtime.a".

Invisible on a long-lived working copy, which already has the archives from
unrelated builds — and only reachable from a clean checkout, which is exactly the
state someone regenerating the public artifact is in. The build now includes the
wrappers and asserts both archives exist before measuring, so the failure is loud
and immediate rather than appearing as a compile error inside a benchmark leg.
