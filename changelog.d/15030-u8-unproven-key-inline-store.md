A `Uint8Array` store whose key or value the compiler cannot prove numeric
(`HEAPU8[p++] = c` with `p` from a `number` parameter, the emscripten string
marshal) tries the header-guarded inline byte store before the runtime store.
A plain-Number integer key in bounds and a plain-Number value store the byte
inline; every other operand keeps the complete `[[Set]]`. The ASCII encode
loop from #14973 runs 12x faster.
