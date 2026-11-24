- **WASI closures with block-compressed source.** On wasm32, a function whose
  retained source is block-compressed was emitted with a 56-byte
  `JsFunctionInfo` instead of the runtime's 64-byte layout, so a call read its
  entry from past the info and ran another body (`tcp_rtt`'s `data` listener
  re-entered its `connect` callback and hung). The compressed-source field now
  keeps its 8-byte alignment on every target.
