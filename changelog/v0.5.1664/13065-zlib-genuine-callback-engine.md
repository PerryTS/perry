Fix one-shot zlib callbacks to expose the genuine working engine with the correct receiver, identity, processed-byte accounting, and inherited event semantics.

- Reuse one codec and async provider; release codec resources and Rust input/output buffers before the completion callback.
- Validate gzip magic only once two bytes are available, preserving short-header error timing.
- Retain the existing GC-owned fallback for exceptional construction, getter, or abandoned after-hook paths.

Validation: 18 canonical candidate calls matched raw Node output; independent verification covered eight baseline/candidate pairs (16 calls) against selected base `76aa5` and Node `26.12.1`, not the full module or latest main.
Local Rust `cfg(test)` compilation and execution remain UNRUN; hosted CI is pending, and the production artifact does not prove Rust tests.
