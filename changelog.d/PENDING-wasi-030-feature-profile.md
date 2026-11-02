Reject Component Model features outside WASI 0.3.0 when composing P3 guests
and checking their binary contracts. Async functions, futures and streams stay
enabled; maps and other unadopted gates are rejected. Core Wasm proposal
requirements remain independent of the WASI interface version.
