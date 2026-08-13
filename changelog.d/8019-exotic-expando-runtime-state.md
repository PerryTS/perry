### Complete the #6759 exotic-expando RuntimeState migration

Date, RegExp, Promise, Map, Set, and Temporal expando values now share the
owning thread's explicit `RuntimeState` with the other hot object-model tables,
instead of resolving a separate table TLS key plus an independent in-use gate.
GC root scanning, owner rekeying, dead-owner pruning, and worker isolation keep
their existing semantics; the shape-tree implementation record now also names
which stronger Phase B/C RFC goals remain separate work.
