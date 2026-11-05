Native Perry can host WASI Preview 1 core modules through `node:wasi` and the
optional WebAssembly archive. The 46 import bindings now call the synchronous
`wasi-common` provider instead of placeholder syscalls. Each WASI import object
owns a collecting native context containing its argument/environment snapshot,
preopens and descriptor table; custom Perry file descriptors are duplicated
into that context. Capabilities are not stored in the shared interpreter store.

`proc_exit` stops the guest immediately, preserves the full unsigned 32-bit
status, and propagates through JavaScript callbacks that re-enter Wasm. Ordinary
import overrides remain ordinary functions. Import exceptions are retained in
existing GC-scanned call frames until the interpreter releases its store borrow.
Missing function imports fail linking and malformed calls do not fabricate
successful results. Guest-memory spans refresh after growth.

Also repair two API blockers exposed by execution: direct and aliased named
`new WASI(...)` now use the registered constructor, and WebAssembly Memory and
Instance initialization defines the internal `buffer`/`exports` properties
without assigning through read-only prototype properties.

Coverage includes syscall/provider tests, preopen confinement and isolation,
stdio ownership, descriptor release, exit sentinels and callback re-entry, plus
Node comparisons and a wasi-sdk 34 libc guest. See `docs/WASI_P1_HOSTING.md` for
qualification and limits. Preview 0 execution, socket capability provisioning,
P1 compiler output, Component Model hosting/WIT and P3 async remain out of scope.
