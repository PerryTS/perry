### Native GC roots reach iOS, iPadOS and tvOS

iOS and iPadOS are aarch64 + Mach-O — the same shape as macOS, which already
worked. They did not work, and the reason was gating rather than anything
architectural: the Mach-O stack-map loader, the unwinder module, the fp-chain
walker and `stack_top` were each `#[cfg(target_os = "macos")]`.

On any other Apple platform that sent the runtime to the no-section stub, so
`loaded_stack_map_section()` returned `None`, the index came out empty, and the
collector ran **with no native roots at all** — silently, on the platforms that
are hardest to debug. The compiler would happily emit the map; nothing read it.

All four gates are now the same predicate: 64-bit Apple, or Linux.
`pthread_get_stackaddr_np` is Apple-wide, not macOS-only, and the `mach2`
dependency was widened to match the code that uses it — declaring it for fewer
targets than the loader compiles on is how this stayed hidden.

**watchOS is refused, deliberately.** `arm64_32` has 32-bit pointers while the
map stores function addresses as `u64` and the runtime does `usize` arithmetic
on them. The compiler rejects the target before emitting a map nothing can
read, and the check is ordered before the `arm64` prefix test so it actually
fires.

Verified by compiling `perry-runtime` for each target: `aarch64-apple-ios`,
`aarch64-apple-ios-sim` and `aarch64-apple-tvos` all build. That is what found
the hole — `stack_top` did not exist on iOS, so the build failed outright
rather than quietly selecting the stub. `aarch64-apple-visionos` still fails in
the third-party `psm` build script, unrelated to this change.

Running on a device or simulator is the verification this does not yet have.
