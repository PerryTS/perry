`Warnings (product)` has failed on every PR opened since #7250 landed. Two denied
warnings in the lazy-barrier-arming code:

- `gc/mod.rs` re-exported `barrier_arming::*` as `pub(crate)`, but every item in
  that module is `pub(super)` — narrower — so the glob re-exported nothing and
  rustc said so. A plain `use` is what the in-module callers actually need.
- `remembered_reconstruct_census`'s only non-test caller is `telemetry.rs`'s
  cycle-JSON emitter, which is itself `allow(dead_code)` without the
  `diagnostics` feature, so a product build saw the function as unused. Now
  carries the same `cfg_attr` as the three sibling sites in `telemetry.rs`.

Neither was caught before merge because `main`'s last `Tests` run predates #7250,
so no run on `main` had ever compiled this code with `-D warnings`.

Three further pre-existing denied warnings were blocking the same job family and
are fixed here too: a redundant `unsafe` wrapper in `pointer_publish_7154.rs`
(#7179), a dead initializer in `oldgen.rs` re-derived by its own loop (#7147),
and `reset_typeof_string_cache_for_test` (#7226), which has no callers anywhere
in the workspace.
