Dropped `rate-limit` and `bundled-dayjs` from
`tests/release/packages/next-app-route/provider/stdlib/Cargo.toml`. Merge
train 231 deleted both features from `crates/perry-stdlib/Cargo.toml` when it
removed the `rate-limiter-flexible` and `dayjs` native bindings, but the
release-package provider still enabled them, so the provider no longer
resolved.

It was invisible to every per-PR gate: the provider is not a workspace member
(it lives under `tests/release/`), so `cargo check --workspace --all-targets`
— the gate that would otherwise catch an undefined feature — cannot see it at
all. It surfaces only in the full tier's release-package smoke, which runs
nightly, on tags and on dispatch.

Found while resolving the `lru-cache`/`commander` removals, which had to touch
the same file to drop `bundled-lru-cache`. The general shape is worth noting
for the rest of the binding-removal campaign: **a feature deletion in
`perry-stdlib` has a consumer outside the workspace**, and `comm -23` between
the provider's feature list and `perry-stdlib`'s defined features is the cheap
check.
