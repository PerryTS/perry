### Fixes
- Implement `bun:jsc.heapStats()` and `heapStats(true)` across static imports, dynamic imports, and `require`. Reports contain Perry's per-thread heap and allocator counters, including type and pinned-cell counts, with JavaScriptCore differences documented. Fixes #9743.
