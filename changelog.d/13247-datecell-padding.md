Fixed a `Date` allocated into reused memory on 32-bit targets (wasm32) being classified as the dead object that occupied it: the cell padding is now zeroed.
