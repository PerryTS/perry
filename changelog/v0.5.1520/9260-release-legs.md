The Windows, musl and old-glibc release build legs no longer fail before they
start. `perry-runtime`'s test build imported two `malloc_trim` counters under a
weaker `cfg` than their declarations, breaking compilation on Windows MSVC; the
glibc-2.31 container never put the mounted Cargo bin directory on `PATH`, so
`rustup` was not found; and the glibc-only `libc::backtrace` pair was selected
for musl targets, where it does not exist.
