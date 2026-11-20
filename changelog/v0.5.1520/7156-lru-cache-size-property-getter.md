Fixed `LRUCache#size` read as a property (`cache.size`) returning `undefined`.
A handle-backed native instance's zero-arg data getters are dispatched as 0-arg
`NativeMethodCall`s only when `is_native_dispatch_member` recognizes the
`(module, class, property)` triple; `lru-cache` had no arm, so a bare `cache.size`
fell through to the inverted default (a plain `PropertyGet`) and the runtime's
handle-property lookup — which has no `size` handler for the compact lru-cache
handle — returned `undefined`. The method-call form `cache.size()` was unaffected
because it routes through the call-expression path to the existing
`js_lru_cache_size` dispatch row. Added the `"lru-cache" => prop == "size"` arm so
the property read now dispatches to `js_lru_cache_size`, mirroring the `blob`
(`size`/`type`/…) and `__disposable__` (`disposed`) getter arms.
