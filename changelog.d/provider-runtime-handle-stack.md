**Provider dylibs share the process runtime's transient GC-handle stack.** On
Apple targets, inlined runtime code inside a stdlib provider could push a handle
into the provider image's private TLS stack and then read it through an
out-of-line method resolved from the runtime dylib. Cross-module `Response` and
`Headers` construction could consequently abort with `runtime handle used after
its scope was dropped`. Handle-stack address resolution now crosses one
interposable runtime-provider symbol, and stale-handle diagnostics release the
stack borrow before panicking so their original backtrace is preserved.
