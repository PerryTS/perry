Fixed native frame symbolization so runtime Rust frames cannot inherit the
name of a preceding builtin getter or function thunk. Error-stack JS lookup
now indexes only codegen-registered bundle functions, suspicious far-offset
matches retain their raw instruction address, and symbol-preserving Unix
builds lazily fall back to the executable's static text-symbol table when
`dladdr` has no name. That process-spawning fallback now requires
`PERRY_STACK_SYMBOLS=1`; the default reports an offline-resolvable image offset.
