### Fix JSON.stringify serialization through proxies (#11867)

Proxy values no longer fall through the native-handle guard and serialize as
`null`. JSON serialization uses the existing proxy operations for `toJSON`,
enumerable own keys and property reads, and uses `IsArray` plus observable
length/index reads for array proxies. Compact output, indentation and both
replacer forms share the proxy traversal, preserving trap order and revocation
and circular-reference errors without introducing object metadata caches.

Adds `test_gap_11867_json_proxy.ts` for transparent and trapped proxies at the
root and inside objects/arrays, nested proxies, array proxies, `toJSON`,
replacers, repeated references, cycles, revocation and throwing traps.
