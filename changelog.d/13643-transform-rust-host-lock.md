The guest component acceptance builds its transform Rust host with `--locked`.
That host lock still held zstd 0.13.3 after turnloop-http moved to zstd 0.14.3,
so `component_check.sh` stopped at build-rust-host on main. The lock now
matches (zstd 0.14.3, zstd-safe 8.2.0, zstd-sys 2.1.2).
