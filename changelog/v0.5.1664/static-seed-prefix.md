A program whose literal objects share their first keys with objects built one
key at a time no longer aborts with "the static ShapeId ... was refused by the
shape mint" (OpenCode's TUI did, at startup). When the incrementally built
objects died, a collection dropped the shared key prefix and cut the literal's
canonical key list off the keys trie, so the literal's module later built a
second key list for the same keys and its compiled shape id no longer matched.
A prefix now stays while a live key list extends it, so equal key lists keep
one array, and one shape, in every thread.
