Use the representation supplied by each root owner when scanning roots.
Generated global and LLVM statepoint slots now keep JSValue tags at rest;
runtime pointer fields share the same header-based marker. Remove ambiguous
heap-word runtime handles and numeric-pointer guessing from precise root marking
and root write barriers. Live allocation metadata confirms current-heap ownership
before reading a header, without restricting roots to the collection census. Legacy untyped shadow frames retain conservative decoding.

### Fixes

- Preserve the numeric encoding of registered Web Streams receivers when rooting native method calls. A stream id encoded as a heap pointer could crash precise root scanning during an incremental collection.

Closure births preserve the existing proof that each cell capture is live. Newly minted cells and forwarded frame cells are stored as tagged JSValue words; entry caches unwrap the payload after reloading its root. Unallocated raw cell homes use a null sentinel consistently with the birth check.

Keep Worker handles, callbacks, termination promises, and async-resource roots on their creating agent. Process-global root providers cannot mark or pin another heap’s allocations; current-heap births after the census and roots in other generations remain valid.
The ownership lookup keeps the copied-minor malloc registry lazy; an inactive registry uses current live allocations without rebuilding it.
