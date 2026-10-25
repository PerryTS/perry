Canonical component frames now retain values through frame-owned traced arrays
instead of keeping a transient native handle scope open. A frame can outlive
its creating scope and finish before or after another frame without removing
the other frame's roots. Array growth publishes the new array head, exceptions
remain rooted, and a late drop after heap teardown releases only native state.
The guest ABI remains version 2; frames stay on their creating agent.

The standalone Rust guest acceptance host now uses Perry's Turnloop providers
and Wasmtime 48.0.7. The Tokio gate also checks these independent fixture
workspaces, lockfiles and Rust sources, with planted violations in its
self-test. The acceptance harness supports an explicit provider Cargo config
without applying that override to unrelated guest fixtures.

Linux qualification covers 45 runtime component tests, one native-class test,
11 async-hook tests, 16 WIT tests, native async/command/HTTP regression suites,
35 Turnloop-based guest acceptance phases, and plain HTTP/TLS with independent
native and guest moving-GC witnesses. The old implementation fails both new
frame-lifetime controls. See the owned guest frame validation receipt for
source and artifact hashes. Generic future/stream bindings and async guest
adapters remain unfinished; the rooting change supplies their lifetime
foundation. Released provider dependencies, other platforms and final packages
remain separate qualification work.
