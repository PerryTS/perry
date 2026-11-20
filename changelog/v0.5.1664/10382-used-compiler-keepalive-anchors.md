The ~490 runtime keepalive anchors are declared `#[used(compiler)]` instead of
plain `#[used]`, so they stop acting as unconditional linker roots.

`#[used]` sets `N_NO_DEAD_STRIP` on Mach-O and `SHF_GNU_RETAIN` on ELF, making
every anchor a `-dead_strip` root. `Cargo.toml` and `optimized_libs/freshness.rs`
argued the cost was bounded, because a linker only pulls in the archive members
that resolve an undefined reference — but `[profile.release]` sets
`codegen-units = 1`, so `perry-runtime` is a single 17.4 MB object. Pulling in
any runtime symbol pulls in all 504 `KEEP_*` statics, and each roots whatever it
points at. Measured with `ld64 -why_live`, a `console.log("yeah")` program
retained `fs`, `child_process`, `dgram`, `tls`, `node_vm`, `bun_ffi`, yoga and
taffy.

`#[used(compiler)]` keeps the static through rustc and LLVM — the reason the
anchors exist, per #6917's revert — while leaving it strippable by the linker,
so the program's own undefined references decide what survives.

Binary sizes fall 13–15% for ordinary programs, 7.3% for ext-routed ones, and
21.7% for `rich.ts` on the auto-optimize shipping path (9.86 MB to 7.72 MB). The
`perry` compiler binary itself drops from 100 MB to 87 MB, since it links the
runtime too.

The archive stays complete: `libperry_runtime.a` exports the same 5611
externally-defined symbols, all 504 `KEEP_*` statics are still present in the
object, and `[no dead strip]` roots fall from 553 to 2.

Four anchors stay plain `#[used]` on purpose — `PERRY_RUNTIME_BUILD_STAMP_EMBEDDED`,
mimalloc's `__DATA,__mod_init_func` entry, OHOS's `.init_array` entry, and
`perry-ext-http`'s `FORCE_LINK_HTTP_SERVER` (#1652), which is intentional linker
retention.

Requires `#![feature(used_with_arg)]` in `perry-runtime` and `perry-stdlib`; both
already require the pinned nightly.
