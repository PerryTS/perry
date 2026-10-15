Const-initialize the remaining console, async-context, trace-output and UTF-8
stream TLS on the production GC-effects witness paths. Console maps keep
RandomState and construct their empty map on the first write; internal UTF-8
stream IDs use the existing const integer hasher. Native containers retain
their existing thread-exit destruction.

Keep the larger cold operation callbacks out of LocalKey::with so its TLS
getter can specialize to the concrete const-initialized store. This removes
opaque getter dispatch without adding storage, caches, or another access path.
Existing const arena and EH-prediction storage needs no further change on the
new main base.

Retain SIMD UTF-8 dispatch and the standard-library reverse-search, hash-map
comparison and string-join implementations. Their residual register-indirect
calls are documented in the lane's archive witnesses; replacing the SIMD or
library algorithms would require a separate performance justification.
