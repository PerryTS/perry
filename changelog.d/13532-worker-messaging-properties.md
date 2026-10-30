### Fixed

- Read MessagePort and BroadcastChannel fields as property values instead of invoking them during lowering, preserving handler identity and receiver-bound calls without a method-name whitelist.
