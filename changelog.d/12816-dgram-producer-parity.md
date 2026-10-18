### Fixed

- Call deferred `dgram.send()` callbacks with an undefined receiver instead of the socket, preserving the socket lifetime capture and callback arguments.
- Populate `address` and `port` on locally produced bind errors, rooting the error and address across property allocation.

Existing node-suite fixtures cover both fixes. Against Node 26.5.1 in FAST/PREBUILT mode with auto-optimization disabled, dgram improves from 41/57 to 43/57 cases; the remaining 14 baseline parity failures and the separate error-callback arity mismatch are outside this change. All 41 prior passing cases and all 55 unaffected cases retain their raw output and exit status.
