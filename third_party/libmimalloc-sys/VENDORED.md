# Vendored libmimalloc-sys 0.1.49

A copy of the crates.io package `libmimalloc-sys` 0.1.49 (mimalloc v2 and v3
C sources included), used through `[patch.crates-io]` in the workspace
`Cargo.toml`. Every upstream file is unchanged except `build.rs`. The upstream
MIT license is in `LICENSE.txt`; `Cargo.lock`, `.cargo_vcs_info.json` and
`.cargo-ok` of the package were dropped.

Perry's change: on x86_64 glibc Linux, mimalloc's calls of 16 libc functions
that Rust code also binds through the GOT (mmap, memcpy, memset,
clock_gettime, `__errno_location`, the pthread key functions, ...) go through
the GOT instead of the PLT. Under lld, Perry's default Linux linker, each PLT
call of such a function gets a second JUMP_SLOT that ld.so binds lazily
(about 400 instructions on its first call; chess +6K per run). GNU ld hides
the cost behind `.plt.got`.

- `perry/perry_noplt_libc.h`: force-included into mimalloc's unity TU with
  gcc. 14 functions get `__attribute__((noplt))`. memcpy and memset are
  builtins whose library calls ignore that attribute, so their symbol is
  renamed (asm label; they stay builtins) to a local stub that jumps through
  the GOT entry, as GNU ld's `.plt.got` stub does.
- With clang (no `noplt` attribute) the unit is compiled with `-fno-plt`.
- `perry/noplt.rs`: after the compile, the build fails if mimalloc's object
  still has an `R_X86_64_PLT32` relocation against one of the 16 functions.
- `build.rs`: two calls into `perry/noplt.rs` and the `mod` line. Nothing else
  (features, defines, flags) changes, and other targets build exactly as
  upstream.

To update: copy the new crates.io package over this directory, keep
`perry/`, `VENDORED.md` and the marked lines in `build.rs`, and re-check the
function list against the JUMP_SLOTs `readelf -rW` shows for an lld link of
chess versus a GNU ld link.
