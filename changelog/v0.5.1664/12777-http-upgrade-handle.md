Fix WebSocket noServer upgrades through HTTP: IncomingMessage registry values now carry HANDLE_TAG, but upgrade_request still required POINTER_TAG and discarded valid requests before the acceptance callback. Decode the canonical native handle and retain the typed registry lookup.

Add a direct bridge regression covering the method, target, version and headers, plus rejection of numeric, forged pointer-tag and dropped handles. The valid noServer echo witness previously timed out while Node printed WSOK; end-to-end validation is required before ready.

The Agent boxing helper also receives live heap socket and closure addresses; canonical boxing preserves POINTER_TAG for those addresses and HANDLE_TAG for registry ids. Previously it tagged the closure as a registry id and listener validation aborted. Initialize standalone fixture layouts and decode native IncomingMessage ids explicitly. The repaired HTTP library passes all219 tests in release before the final main rebase.

Initialize runtime layouts in the standalone turnloop client integration before constructing its real request and Agent records; retain all end-to-end transport counter and wire assertions.

Accept canonical native handles in options.agent while preserving existing embedding carriers and typed registry validation. The end-to-end pooled client fixture exposed the stale pointer-only extractor; require correct Agent resolution and reject a dropped handle.
