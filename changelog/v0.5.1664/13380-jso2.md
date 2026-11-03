### Performance

- Sort proven pure numeric orderings with reusable payload storage for dense i32 arrays, and admit non-NaN floating-point values and plain own numeric object keys to stable radix sorting. Preserve comparator effects and unsupported receiver/key semantics through the existing fallback.
- Return retired array-growth payload pages to the OS while preserving forwarding aliases, and initialize dense replacement slack after retiring the old backing to reduce peak resident memory.
- Initialize fresh array layouts from their allocator-proven brand and reset header, avoiding repeated receiver checks during startup.
