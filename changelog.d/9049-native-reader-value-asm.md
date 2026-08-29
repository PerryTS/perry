The native IR reader accepts value-returning inline asm.

It handled only the void inline-asm barrier; the value-returning form fell
through to the ordinary-call callee search and failed with `call without
callee`. The hot-TLS thread-pointer read emits exactly that form —

    %r = call i64 asm sideeffect "mrs $0, tpidrro_el0", "=r"() "gc-leaf-function"

— so on macOS aarch64 any function taking the inline allocation fast path failed
native IR construction, killing the build. The textual transport compiles the
same IR fine, and small modules use it by default, which is why no suite saw
this: only large codegen-unit-split modules (or an explicit
`PERRY_LLVM_INPROCESS=native`) reach the reader. That is the dialect reader's
closed-set design working as intended — a new emission form fails loudly at
construction — but the form had no arm.

Two parsing details the reader test pins: the arg list is found after the
CONSTRAINTS' closing quote (the 4th quote), not the last quote on the line,
because the emitter appends a quoted `"gc-leaf-function"` attribute after the
args; and the rebuilt callsite carries the same structural `gc-leaf-function`
marking as the void barrier, since perry-emitted asm never re-enters the runtime
and RS4GC must not statepoint-wrap it (#8121).
