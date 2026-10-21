# Native host networking interoperability

This Rust-authored component imports the pinned WASI 0.2.12 socket interfaces
and exports a small typed network probe. A native Perry host exercises protocol,
address and port denials; connected and unconnected UDP exchanges with a local
Python echo server; TCP connection grants and traffic; DNS permission; and fresh-instance isolation. DNS uses only
localhost, an IP literal and a syntactically invalid name.

The fixture covers the component ABI and configured host providers. It does not
establish UDP support in Perry-authored WASI guests (`node:dgram` remains excluded
from the WASI runtime), host-I/O cancellation, HTTP or native async P3 behavior.
Synchronous polling can block; the enclosing harness must bound execution.

`deps/` contains unmodified WIT definitions from the lockfile-pinned
`wasmtime-wasi` 48.0.5 crate, with its license and per-file provenance hashes.
The fixture uses the same pinned `wit-bindgen` as the existing component probes.
