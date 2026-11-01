Add a distinct `--target wasip3` command/link profile using its own
`wasm32-wasip3` runtime archive and wasi-sdk's native 0.3 component link. Keep
the P2 optimisation adapter out of P3 links. Full guest future/stream support
and release qualification remain unfinished.

P3 IR retains Perry's wasm32 shadow roots and in-process optimisation, then
uses SDK Clang 23 to emit the newer thread-context ABI. Fingerprint that
emitter in object caches and keep P3 whole-output caching disabled pending
SDK sysroot/linker fingerprinting. The P3 command acceptance profile grants a
bounded one-billion-instruction budget for the full runtime's startup.
