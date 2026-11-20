### Tests

**Every Apple target Perry builds for is now pinned in the GC-map emitter, with the address width its ABI actually uses.**

`gc_map.rs` had one ILP32 test (`arm64_32-apple-watchos`) and one LP64 test
(`arm64-apple-ios`). Those pin two triples; nothing pinned the *set*, so adding
a target without deciding its address width failed at someone's link step
rather than in CI. `every_apple_target_is_accepted_with_its_own_address_width`
covers macOS, iOS, iOS-sim, tvOS, visionOS, watchOS (both `arm64` and the ILP32
`arm64_32`) and x86-64 macOS, asserting each is not refused and emits the right
width.

Written while establishing what actually blocks watchOS and visionOS, which is
worth recording because it is **not Perry**:

`cargo check -p perry-runtime` succeeds on stable for macOS, iOS, iOS-sim and
tvOS, and for watchOS and visionOS with any feature set that excludes
`dyn-eval`. With `dyn-eval` — which is in `default` — both fail in `psm`, three
crates away (`dyn-eval` → perry-parser → swc_ecma_parser → stacker → psm). psm
selects its assembly with

```c
#if defined(CFG_TARGET_OS_darwin) || defined(CFG_TARGET_OS_macos) || defined(CFG_TARGET_OS_ios) || defined(CFG_TARGET_OS_tvos)
```

which omits `watchos` and `visionos`, so both fall through to the ELF branch and
emit `.type`/`.size` — directives the Mach-O assembler rejects. Verified by
patching that one line locally: with it, both targets compile with full default
features, including `dyn-eval`.

Two consequences worth stating plainly:

- **watchOS and visionOS worked before and regressed on 2026-07-18**, when
  `dyn-eval` was added to `default` (#6584). Nothing about those platforms
  changed; a transitive dependency arrived. The release workflow's watchOS note
  blames a different cause (a `ring` pointer-size assertion, which is real but
  specific to the ILP32 `arm64_32` triple).
- **No release has caught it.** `release-packages.yml` builds these targets with
  default features, and its last successful run was 2026-07-04 — before the
  regression. The two attempts since were cancelled.

Programs that never call `new Function` with a runtime-built body are
unaffected: the auto-optimize path enables `dyn-eval` per program.
