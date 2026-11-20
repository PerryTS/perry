Fixed native Response dispatch for imported userland fetch functions, including
pacote's npm-registry-fetch path. The native-instance pass inferred a native
Response from an external function named `fetch`, then sent minipass-fetch's
JavaScript Response to the native body reader, which rejected it with `Invalid
response handle`. Native Response results are now identified by the fetch HIR
intrinsics rather than a function name, preserving the userland `json()` method.

Added a dependency-free imported-fetch gap regression and a HIR regression test.
