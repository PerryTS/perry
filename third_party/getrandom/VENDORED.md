# getrandom WASI 0.3 backports

These copies retain the workspace's existing 0.2.17 and 0.3.4 API versions.
Registry archive hashes are recorded in `upstream.json` and checked against
Perry's original lockfile before extraction. Upstream MIT and Apache licenses,
published manifests, tests and source provenance are retained. Registry status
files and package lockfiles are omitted; Perry's workspace owns resolution.
Two upstream prose lines have trailing whitespace removed so the repository's
diff checks can run without exempting vendored files.

The backend changes select a new backend on `wasm32-wasip3`. It imports
`wasi:random/random@0.3.0::get-random-u64` and retains the existing alignment,
partial-buffer and error APIs. The scalar manual binding follows the upstream
[getrandom 0.4.3 backend](https://github.com/rust-random/getrandom/blob/v0.4.3/src/backends/wasi_p2_3.rs).
Its component type metadata comes from the linked WASI standard library/libc,
as in that upstream backend. No dependency or backend is changed for other
targets. In particular, the P3 backend does not call the P1 random API.

Three integer constants also use warning-free equivalent spellings: the
0.2.17 uninitialized `usize` sentinels use `!0`, preserving its upstream MSRV,
and the 0.3.4 futex wake count uses `libc::c_int::MAX`. Their values and types
are unchanged. This keeps the path dependencies compatible with Perry's
warnings-as-errors checks without suppressing dependency diagnostics.

This backport is needed because dependencies still require getrandom 0.2/0.3;
changing those requirements to 0.4 would also change their public APIs.
Qualification must include actual P3 execution and current native/P1/P2 checks;
the presence of these sources alone does not qualify the P3 runtime profile.
