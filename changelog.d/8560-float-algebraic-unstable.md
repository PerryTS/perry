**`cargo-test` cannot compile on `main`: `perf_histogram.rs` uses the unstable `float_algebraic` API without a feature gate.**

`main` (`4ee1d04b5`) fails to build its test targets:

```
error[E0658]: use of unstable library feature `float_algebraic`
   --> crates/perry-runtime/src/perf_histogram.rs:277:31
    |
277 |                 let dev = dev.algebraic_sub(mean);
```

#8550 introduced `algebraic_sub` / `algebraic_mul` / `algebraic_add` into the stddev reduction, with a comment describing them as "Rust 1.98's algebraic_* float methods". They are not stable in 1.98 — `float_algebraic` is still an unstable *library* feature, so it needs `#![feature(float_algebraic)]` regardless of channel. `perry-runtime` declares no such gate, nothing sets `RUSTC_BOOTSTRAP`, and the repo has no `rust-toolchain` file, so the call sites fail to compile on stable **and** on the nightly that same commit pinned. That is why the breakage shows up as `cargo-test` dying before any test runs rather than as a test failure.

The reassociation those methods enable was only ever an optimization, and the comment that shipped with them said so — Node's HdrHistogram-based stddev is a bucketed approximation that nothing requires bit-exact. Plain arithmetic is therefore behaviour-preserving here, and it keeps the crate building on stable, which matters while there is no pinned toolchain file in the tree for contributors to inherit.

If the crate later moves to a *required* nightly, the intent can be restored behind an explicit `#![feature(float_algebraic)]`; the replacement carries a comment saying so.

Verified: `cargo check --workspace --tests` exits 0 with zero errors (it fails on `main`), and `cargo test -p perry-runtime perf_histogram` is 9/9.
