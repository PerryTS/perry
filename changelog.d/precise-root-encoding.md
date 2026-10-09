Use the representation supplied by each root owner when scanning roots.
Generated global and LLVM statepoint slots now keep JSValue tags at rest;
runtime pointer fields share the same header-based marker. Remove ambiguous
heap-word runtime handles and classifier probes from precise root marking and
root write barriers. Legacy untyped shadow frames retain conservative decoding.
