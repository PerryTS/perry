CI on `main`: the `RecordArrayHeader` size assertion pins four `u32` counts
plus the buffer pointer instead of 24 bytes, so the runtime builds for
`wasm32-wasip1` again (`wasi-execution`); `crates/perry-runtime/src/lib.rs` is
formatted; eleven raw-handle ceilings the ratchet reports as matching nothing
are deleted (none raised); and two wasm integration expectations follow
`console.log`'s one-line output from #14625.
