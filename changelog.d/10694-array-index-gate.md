Plain-array element reads and writes no longer probe the buffer and typed-array
registries. A `GC_TYPE_ARRAY` header can never be either, and the iteration
helpers already gated on that; the indexing path did not. On a `tsc --noEmit`
of a two-line file this halves `is_registered_buffer` probes, 79.7M to 39.8M
(#10694).
