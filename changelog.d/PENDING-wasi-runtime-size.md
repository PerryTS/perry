Build WASI runtime archives with size optimization for the runtime crate and
the existing ThinLTO settings. A controlled P1 WebSocket build shrinks from
5.87 MB to 4.75 MB with the same features; native build profiles retain their
existing settings. Preserve the checkout's unwind options.
