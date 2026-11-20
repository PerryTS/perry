`Object.defineProperty(fn, "prototype", { value })` with the default
attributes (writable, not enumerable, not configurable) now keeps the value
that `fn.prototype` reads. The define no longer records those attributes in
the function's own-key bag, where they added a valueless `prototype` key that
hid the value in the function's `prototype` slot. A `[[Set]]` of
`prototype` through a trap-less Proxy, which defines the existing own
property, is fixed with it (test262
`built-ins/Proxy/set/trap-is-undefined-target-is-proxy.js`).
