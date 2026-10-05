Fixed `crypto.hash(...)` called through a namespace value: the hash object is
now rooted across its `update` call, so a collection in that window cannot
reclaim it before `digest` runs.
