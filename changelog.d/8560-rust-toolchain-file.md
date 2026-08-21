### Fixed

- Add `rust-toolchain.toml` pinning the same `nightly-2026-08-20` that #8550
  pinned across the 23 CI workflows. Those inputs govern only jobs that use
  `dtolnay/rust-toolchain`, so local builds, bisects and any script invoking
  cargo directly still resolved to the default toolchain and failed to compile
  `perry-runtime`'s `float_algebraic` uses with E0658 while CI stayed green.
