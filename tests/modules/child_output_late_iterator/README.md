# Child output: async iteration after EOF

Run `node scripts/test-child-output-late-iterator.mjs` with `PERRY_BIN` pointing
to a compiler and `PERRY_RUNTIME_DIR` pointing to matching static libraries.
For the Wasm-enabled archive configuration, also set `PERRY_TEST_WASM=1`.

The runner uses the executing Node as an explicitly selected real child, checks
the Node oracle, and compiles/runs the same fixture at O0, Os, and Oz. Each
compile and execution has a timeout, and failures retain their diagnostic files.
No application bundle, network, credentials, or downloaded npm dependency is
needed.

Both output pipes must emit EOF and expose `readable === false` and
`readableEnded === true` inside their end callbacks. Only after child close does
the fixture first pull an iterator created before EOF, then create and consume
fresh iterators. All must finish without waiting for a second end event.

Before the fix, the native program fails the visible EOF-state assertion; without
that assertion, it waits forever on the first late pull. Runtime unit tests also
cover pending empty pulls and preserving chunks buffered before EOF.
