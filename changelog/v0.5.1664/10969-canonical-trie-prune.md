Fix quadratic retirement of weak canonical-key trie nodes. Pruning now retires the dead batch, filters each edge bucket and collision chain once, orphans surviving children, and only then recycles node IDs. This preserves weak GC rewriting and the one-probe interning path without adding a side table or thread-local.

A test-only edge counter bounds a 20,000-node / 15,000-death prune to linear work; regression coverage also checks collision chains, surviving descendants, and safe ID reuse.
