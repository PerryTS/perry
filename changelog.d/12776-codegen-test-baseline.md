Refresh codegen test fixtures for the current receiver/layout probes and archive-derived GC effects. Populate requested_modules in synthetic integration modules. Keep live-root, exact getter-bit, array-kind, forwarding-flag, complete fallback, and receiver-proof checks against current IR.

Delete tests and test-only constants for the deliberately removed per-site PIC/front/cache design; no removed cache, side table, or GC-effects registry is restored. Production lowering is unchanged.

The integration inventory also refreshes image-string addresses, guarded numeric argument helpers, array births, method metadata filtering, complete Get/Set fallbacks, and prototype-census fixture setup. Add a dynamic AppWindow frame-name case to preserve short-string decoding safety. Remove tests dedicated to retired compiler PIC/bookkeeping paths; runtime layout-probe tests own the current slot/barrier contract.
