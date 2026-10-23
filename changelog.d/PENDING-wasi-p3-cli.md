Add optional WASI 0.3.0 CLI providers using the context’s Turnloop-backed native stream views. Canonical streams carry bounded byte chunks, and separate result futures report completion or typed I/O errors. Accepted writes retain flush backpressure; Store retirement remains the hard-cancellation boundary.

Test large binary transfers and result-future ownership through a real Wasm component, inherited native stdio, exact output capacity, closed defaults, broken pipes and delayed flush errors. P3 filesystem/socket/HTTP providers and Perry’s TypeScript async/GC bridge remain pending.

Validation: all 243 provider tests pass in Linux development and production panic-abort builds, with all-target Clippy, doctests and the Tokio-free graph passing. See `docs/validation/wasi-p3-cli-20261023.json`.
