Ran `cargo fmt -p perry-runtime` over the five files #10836 touches, which were
`cargo fmt --all -- --check` red at the PR's own head: 10 hunks across
`gc/tests/handle_bound_method_name.rs`, `object/mod.rs`, `text.rs`, `timer.rs`
and `timer/tests_inline.rs`.

Confirmed this is the PR's own red and not a local-toolchain artifact before
touching anything, because the distinction decides whether reformatting is a
fix or a new divergence: `origin/main` is `cargo fmt --all -- --check` **clean**
under the identical binary (`rustfmt 1.10.0-nightly f7d782a3be 2026-08-19`)
while #10836's pristine head is red with those same 10 hunks. A one-day delta
from the `nightly-2026-08-20` pin in `external-tools.json` cannot explain a
result that reproduces on one tree and not the other.
