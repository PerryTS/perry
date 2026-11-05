Add native `runComponentCommand` for one-shot WASI P2 commands. Guest return and explicit exit become status codes; traps and configuration failures stay catchable, and guest exit never terminates the Perry host. Each command uses an independent capability context and a shared initialization/execution fuel budget. Validate `wasi:cli/run` before initialization and release the store on every exit path.

The optional component-host ABI advances to 3. Linux qualification passes five command/FFI tests, 20 runtime component tests and five Rust-to-Perry acceptance phases with capability isolation and actual moving GC. The execution receipt records source revisions and artifact/log hashes.

Refresh generated component API documentation and declare command options optional.

Accept inline short strings at the shared component value boundary, including
dynamically converted command arguments such as `String(exitCode)`.
