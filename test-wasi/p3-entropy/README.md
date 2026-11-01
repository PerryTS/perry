This standalone fixture checks the two pinned getrandom backports on
`wasm32-wasip3`. Build `--bin entropy --target wasm32-wasip3` with the pinned
Rust toolchain and wasi-sdk 34's component linker. Run the result through the
Turnloop command host. Both RNG versions must fill all 256 alignment/length
combinations without overwriting adjacent bytes.

Build `--bin p3-contract` for the native host and pass it the resulting
component. It parses the actual top-level import/export sections and requires
only WASI 0.3.0 interface imports, including randomness, plus the 0.3.0 command
export. It rejects P1 core modules and P2 or mixed-version component contracts.
It does not replace engine validation or command execution.
