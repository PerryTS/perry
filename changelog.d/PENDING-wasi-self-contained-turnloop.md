Select pinned repository copies of the two Turnloop crates that require provider
repairs. Normal locked builds now use the exact Rust sources qualified for WASI
networking, async calls and concurrent HTTP without external Cargo overrides.
Keep published `turnloop-io` alpha.9, whose sources match the qualified copy, and
preserve the no-Tokio policy.

Include registry checksums, complete selected Rust source hashes, MIT licenses
and replacement guidance. Retain upstream module layout, with a documented
file-size exemption for the vendored driver. Development and production
panic-abort tests exercise providers, component I/O, the component engine and
WIT generation through the normal lockfile. Other platforms and final release
packages still require qualification.
