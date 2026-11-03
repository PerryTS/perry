---
category: Runtime
title: Harden WebAssembly host bindings
---

`WebAssembly.instantiate(Module)` now returns the specified Promise while
preserving its optional-imports dispatch, Wasm table function wrappers release
their host handles when collected, and a JavaScript import that re-enters
WebAssembly no longer aliases the shared host store: nested export calls,
funcref-table calls and standalone Memory/Table/Global operations borrow
through the active import's wasmi `Caller` (Emscripten `invoke_*`/`dynCall`
and imports calling exports such as `_malloc` keep working). Overlapping store
access outside an import boundary is refused.
