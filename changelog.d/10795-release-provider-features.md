Dropped `bundled-moment` and `bundled-exponential-backoff` from
`tests/release/packages/next-app-route/provider/stdlib/Cargo.toml`, which the
same change deletes from `crates/perry-stdlib/Cargo.toml`.

That provider is **not a workspace member** — it lives under `tests/release/` —
so `cargo check --workspace --all-targets`, the gate that would normally catch
an undefined feature, cannot see it at all. It breaks only in the full tier's
nightly release-package smoke.

This is the second occurrence: merge train 231 deleted `rate-limit` and
`bundled-dayjs` and left the same file unresolvable. With more binding removals
queued, the pattern is now checked rather than remembered —
`check_nonworkspace_features.py` derives perry-stdlib's defined features and
every non-workspace consumer's requested ones and refuses the difference. It
was proven to discriminate: clean on the fixed tree, and on the pre-fix tree it
names both features and exits 1.
