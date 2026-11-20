### perf(runtime,codegen): keep populated-delete ICs stable per key

Owned ordinary objects now keep one ShapeId across tombstone delete/re-add
epochs. Read and write IC hits validate the cached value slot against
`TAG_HOLE`, while a guarded re-add lane appends the key to preserve enumeration
order and re-primes both megamorphic caches. Squeeze still publishes a fresh
immutable shape, and flag-off in-place compaction retires growth-era prefix
tokens before shifting keys.

On Linux, a min-of-seven interleaved run of `bench_popdel.ts` against the exact
main tip improves from 329 ms to 57 ms (Node: 21 ms, Perry: 2.71x Node), while
the six #9029 differentials remain byte-identical in both tombstone modes.
Fixes #9064.
