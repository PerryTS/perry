Fix iterator helpers retaining stale helper, source, and callback addresses across moving nursery collections. Runtime handles now keep lazy-helper state writes, callback results, `toArray` output, terminal callbacks and `reduce` accumulators, and iterator-wrapper arguments current after allocation or user callbacks.

Eight copying-nursery regressions assert actual relocation, stored edges, yielded values, and callback order. Restoring the missing helper root reproduces the field-3 out-of-bounds warning and fails the live drop-counter assertion.
