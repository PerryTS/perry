Add native `runComponentCommand` for one-shot WASI P2 commands. Guest return and explicit exit become status codes; traps and configuration failures stay catchable, and guest exit never terminates the Perry host. Each command uses an independent capability context and a shared initialization/execution fuel budget. Validate `wasi:cli/run` before initialization and release the store on every exit path.

The optional component-host ABI advances to 3. Add command/FFI regressions and a Rust-to-Perry acceptance fixture with capability isolation and moving GC. Execution qualification is pending.

Refresh generated component API documentation and declare command options optional.
