Android and OpenHarmony programs that use a closure link again. These targets
always link the program as a shared object, but codegen treated an
`executable` compile for them as an ELF executable and addressed thread-locals
local-exec, which `ld.lld` rejects under `-shared`
(`R_AARCH64_TLSLE_ADD_TPREL_HI12`, `R_X86_64_TPOFF32` against
`PERRY_AGENT_PTRS` and `PERRY_INLINE_ARENA_STATE`). Their emitted code now
reaches the runtime agent block and inline-arena state through the runtime
accessors, and the in-process backend emits emulated TLS for these triples,
the model rustc builds their runtime archives with, so every thread-local
(the runtime's and the program's own) binds through `__emutls_v.<name>`.
Linux and macOS executables are unchanged. Tests cover the TLS model per
target and the emitted access for both Android and OpenHarmony triples.
