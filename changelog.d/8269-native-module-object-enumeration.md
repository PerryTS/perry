### fix(runtime): enumerate native-module namespace values

`Object.entries` and `Object.values` now expose a native module's public export
surface instead of its internal `__module__` storage sentinel. Enumeration
uses the same virtual key list as `Object.keys` and resolves each live value
through the namespace property getter, including constants and callable
exports. The receiver and intermediate arrays stay rooted across allocations
that can trigger a moving collection. Fixes #8235.
