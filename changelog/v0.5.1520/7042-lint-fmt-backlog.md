Applied `cargo fmt` to four files that had drifted out of rustfmt compliance on
`main`: `lower_call/native/mod.rs`, `global_this/install_static.rs`,
`commands/check.rs`, and `commands/deps.rs`.

Pure formatting — line breaking only, no semantic change. These were failing the
`lint` gate's `cargo fmt --all -- --check` step, which sits behind the stale
public-benchmark-baseline step and so was invisible until the baseline failure
was investigated.
