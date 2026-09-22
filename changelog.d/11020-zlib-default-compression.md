Default gzip and deflate compression no longer degrade when Perry's CLI and
the external zlib wrapper are built together. The CLI's ZIP dependency enabled
flate2's `zlib-rs` backend through Cargo feature unification, which made the
issue's 4 MiB payload compress to 31,388 bytes instead of roughly 16 KiB.

ZIP keeps its existing codec support while selecting the backend-neutral
flate2 feature, allowing Perry's normal Rust backend to serve `node:zlib`.
Regression coverage checks synchronous, callback, and promisified gzip calls.
