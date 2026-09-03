# llhttp differential fixtures (#9611)

`llhttp.wasm` / `llhttp_simd.wasm` are the two WebAssembly builds of
[llhttp](https://github.com/nodejs/llhttp) that [undici](https://github.com/nodejs/undici)
ships and loads at runtime (undici picks the SIMD build when the engine
supports it). Both are MIT licensed, like undici and llhttp themselves.

They were extracted verbatim, in this order, from the base64 blobs embedded in
a Claude Code bundle -- the build whose wasm inventory issue #9611 was written
from, where llhttp is the only real WebAssembly on the network path. The names
are undici's own filenames applied in bundle order; nothing in the test depends
on which of the two is the SIMD build, and both produce an identical trace.

    llhttp.wasm  sha256 b96063c7ce14045f91f17489d8b30a2bf5129308bd801d7dde715579d16d0e21
    llhttp_simd.wasm  sha256 989f2025b23e92ae5093ceb357093df7bdf2e1e7f1f1bf383b0a4dc69a78151d

`driver.ts` drives them the way undici drives them: a windowed `Uint8Array`
over the engine's linear memory is filled with the socket chunk,
`llhttp_execute` runs, and the parser calls back into JS. Every callback span
is read BOTH out of linear memory and through undici's own trick of mapping
the wasm pointer back into the source chunk, and the two must agree.

`expected.txt` is the trace from `node --experimental-strip-types driver.ts`,
byte for byte, on the Node pinned in `.node-version`. It is the oracle: perry
must reproduce it exactly. Regenerate with

    node --experimental-strip-types driver.ts llhttp.wasm > expected.txt
