The `link::wasi` golden link-argument tests match the WASI link line after the
switch to native WebAssembly exception handling (#13337), which no longer links
`-lsetjmp`. Both tests failed on `main` in the required `wasi-execution` job.
