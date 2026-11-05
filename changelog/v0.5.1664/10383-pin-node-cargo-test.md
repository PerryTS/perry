Pin Node from `.node-version` in the `cargo-test` and `cargo-test-perry` jobs.
Neither ran `setup-node`, so both inherited whatever Node the `ubuntu-latest`
image ships — 22.23.2 against a 26.5.1 pin — and
`bun_embedded_compression::standalone_compressed_asset_regression` failed the
`process.versions.node` assertion that `scripts/test-bun-embedded-compression.mjs`
opens with, taking the whole job red on `main`. That assertion is the point of
the test: the loader and byte behaviour it gates on is version-specific.

Both jobs are pinned, not just the one observed failing — which shard a
node-backed test lands in is decided by `ci_cargo_test_shard.py`, so pinning one
would move the failure with the sharding instead of removing it. Pinned from the
file rather than a literal, because `check_node_version_consistency.py` requires
every literal `node-version:` to be a registered exemption.
