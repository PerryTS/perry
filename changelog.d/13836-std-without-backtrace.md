Every perry build compiles the Rust standard library from source without its
`backtrace` feature (`.cargo/config.toml` `[unstable] build-std`, `rust-src` in
`rust-toolchain.toml`). The prebuilt std's default panic hook linked its DWARF
symbolizer (gimli, addr2line, miniz_oxide, rustc-demangle) into every program:
a hello world loses 170 KB of `.text` and 191 KB of stripped file on Linux
x86-64, every runtime archive about 10-20 MB of std debug info. A Rust panic in
the runtime prints the same message, location and exit status; `RUST_BACKTRACE`
no longer prints frames. Tests build with `panic=abort` (`panic-abort-tests`),
as every shipped build already did. `scripts/check_std_symbolizer.sh` fails CI
and release packaging when an archive carries std's symbolizer again.
