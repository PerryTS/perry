### Fixed

- Correct JavaScript `AsyncResource` receiver behavior: four methods now throw plain `TypeError` for nullish or detached receivers. `bind` preserves meaningful function-receiver behavior and callback-validation precedence, using the call-time receiver and deferred scope-method lookup for implicit callback `this`, versus eager method capture for explicit callback `this`.
- Keep the native API, resolver, binder, trampoline and context restoration unchanged. Generic JavaScript binding uses ordinary traced captures, not a new context store. Extract five JavaScript thunks into a private child module to keep the parent below 2,000 lines without code compression.
- Verify the existing five-row and new 26-row receiver regressions turn green after red, with prior controls preserved. This is a bounded receiver repair, not whole-module `async_hooks` parity or a universal GC-safety claim; remaining HTTP, NET, zlib and watcher failures and 18 `NOTRUN` cases remain separate.
