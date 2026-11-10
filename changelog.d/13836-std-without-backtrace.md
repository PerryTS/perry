Every perry build compiles the Rust standard library from source without its
optional backtrace support (`.cargo/config.toml` `[unstable] build-std`,
`rust-src` in `rust-toolchain.toml`). Compared with rustup's prebuilt std,
which is built with the features `backtrace`, `panic-unwind` and
`compiler-builtins-c`, this std is built with none of them: no DWARF
symbolizer (`backtrace`), no unwinding panic runtime (every perry build is
`panic=abort`; `panic-unwind` was never linked), and compiler_builtins' Rust
intrinsics without compiler-rt's C copies (`compiler-builtins-c` needs the
compiler-rt sources, which `rust-src` does not ship). std has no other
feature that the prebuilt build enables. The prebuilt std's default panic
hook linked its symbolizer (gimli, addr2line, miniz_oxide, rustc-demangle)
into every program: a hello world loses 170 KB of `.text` and 191 KB of
stripped file on Linux x86-64, every runtime archive about 10-20 MB of std
debug info. A Rust panic in the runtime prints the same message, location
and exit status; `RUST_BACKTRACE` no longer prints frames. Tests build with
`panic=abort` (`panic-abort-tests`), as every shipped build already did.
`scripts/check_std_symbolizer.sh` fails CI and release packaging when an
archive carries std's symbolizer again. Native-library crates a project
builds itself are compiled by the project's own cargo and std, as before.
