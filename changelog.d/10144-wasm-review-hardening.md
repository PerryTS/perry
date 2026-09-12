---
category: Runtime
title: Harden WebAssembly host bindings
---

`WebAssembly.instantiate(Module)` now returns the specified Promise while
preserving its optional-imports dispatch, Wasm table function wrappers release
their host handles when collected, and imported callbacks can no longer
re-enter the shared host store unsafely.
