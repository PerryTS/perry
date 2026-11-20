### wasm: exported calls no longer copy the whole linear memory both ways

`WebAssembly.Memory.prototype.buffer` now exposes the engine's linear memory
directly instead of a copy synchronised around every exported call, so a JS
write through `new Uint8Array(memory.buffer)` lands in wasm memory and a wasm
write is visible to JS with nothing copied in either direction. A
`memory.grow` still hands out a fresh `ArrayBuffer` and detaches the old one,
as node does. Exports also resolve to a handle once per instance rather than
by name on every call. A call is now flat in memory size — 169 ns at 4.3 MiB,
down from 162 µs (#9611).
