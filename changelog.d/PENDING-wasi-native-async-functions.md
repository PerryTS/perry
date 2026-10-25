Add native canonical async component calls and Promise-returning TypeScript
host bindings. Preserve async function declarations in WIT manifests and reject
calling-convention mismatches before instantiation. Resume imports on the owning
agent with fresh callback frames, trace suspended JS state through moving GC,
preserve rejection identity and submission context, and retire suspended Stores
on cancellation or failure. Keep successful instances reusable and reject
overlapping calls on the same component.

Run JS imports and destructors on the embedding stack after Wasmtime yields,
and retain resource wrappers while queued arguments and import results await
native ownership transfer. Linux native engine, actual compiled TypeScript,
moving-GC and P2 regression qualification is recorded in
`docs/validation/wasi-native-async-functions-20261025.json`.

Link Turnloop-backed WASI 0.3.0 providers in the optional engine and extend its
versioned boundary to ABI 7. Matching compiler/runtime/engine archives must be
rebuilt together. Generic WIT futures/streams, async Perry guest adapters,
P3 command/HTTP entry points and remaining platform/package qualification are
still open; this change does not establish full WASI 0.3 support.
