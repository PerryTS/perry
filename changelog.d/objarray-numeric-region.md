### Compiler

- Admit statically bounded object-array element windows into loop regions. Share the checked and admitted element binding before one receiver tail, retain one live shape decision, and use the static shape's F64 lanes to prove numeric accumulator updates under guarded entry and re-entry.
- Combine integer counter range admission with the existing array-region entry instead of copying another pair of loops. Range misses retain Number counter storage across the signed 32-bit boundary.
- Emit read-only static receiver guards directly into their verified field-load arm; retain checked behavior for other shapes, accessors, proxies, holes, nonnumeric values, and coercing bounds.
- Supply the fresh object-spread shape interner's missing integrity argument, restoring builds of the starting revision.
