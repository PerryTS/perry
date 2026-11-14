CI on `main`: the mechanical reds are fixed — `cargo fmt` on three files, the
two `-D warnings` errors (an unused `mut` in `unit_graph.rs`, a function item
cast straight to `usize` in `global_this.rs`), the wasm32 build of
`perry-runtime` (`perry_life_depth` shifted a 32-bit `usize` by 32), and two
stale file-size exemption counts.
