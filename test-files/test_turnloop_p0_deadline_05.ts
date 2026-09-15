// Node-compatible API clamps 0.5 ms to 1 ms; Rust tests cover a true 500 us wait.
setTimeout(() => console.log("deadline 0.5 hit"), 0.5);
