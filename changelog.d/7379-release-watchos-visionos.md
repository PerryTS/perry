### Fixed

**watchOS is back in the release bundles, and visionOS stops failing the release build.**

Two separate problems, one of which had been latent since 2026-07-18.

**visionOS was broken and no release had caught it.** `dyn-eval` joined
`perry-runtime`'s `default` feature set in #6584, and it reaches `psm` three
crates down (`dyn-eval` → perry-parser → swc_ecma_parser → stacker → psm). psm
selects its assembly with

```c
#if defined(CFG_TARGET_OS_darwin) || defined(CFG_TARGET_OS_macos) || defined(CFG_TARGET_OS_ios) || defined(CFG_TARGET_OS_tvos)
```

`watchos` and `visionos` are absent, so both fall through to the `#else` **ELF**
branch and emit `.type`/`.size`, which the Mach-O assembler rejects. Nothing in
Perry is involved. `release-packages.yml` builds those targets with default
features, and its last successful run was 2026-07-04 — before the regression —
so this would have surfaced at the next release. Fixed upstream as
[rust-lang/stacker#152](https://github.com/rust-lang/stacker/pull/152); until
that lands, the two platforms build `default` **minus `dyn-eval`** and the only
capability they lose is runtime `new Function` over a string body.

**watchOS was dropped for a reason that never applied to the triple it ships
on.** The v0.5.888 note blamed ring 0.17.14's pointer-size assertion — real, but
specific to the **ILP32** `arm64_32-apple-watchos` triple (32-bit pointers,
64-bit registers). `aarch64-apple-watchos` is LP64; ring builds for it, and so
does `perry-ui-watchos`. The LP64 device triple and its simulator are restored
across all three sites that needed it — the `build:` cross-compile loop, the
`build-cross` matrix, and the bottle staging step. `arm64_32` stays out until
ring is fixed or pinned.

Verified per target with `cargo check` on stable and the exact feature list the
workflow now passes:

| target | runtime + `-static` | stdlib + `-static` | UI crate |
|---|---|---|---|
| `aarch64-apple-watchos` | ✅ | ✅ | ✅ |
| `aarch64-apple-watchos-sim` | ✅ | ✅ | ✅ |
| `aarch64-apple-visionos` | ✅ | ✅ | ✅ |
| `aarch64-apple-visionos-sim` | ✅ | ✅ | ✅ |

`library_search.rs` already maps `_watchos` / `_watchos_sim`, so no compiler-side
change was needed. Three stale comments claiming watchOS was dropped, and one
asserting the device triple is `arm64_32`, are corrected — the second is what
kept the platform out for months.
