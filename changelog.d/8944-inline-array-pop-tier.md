### Changed

- `arr.pop()` on an Array-admitted receiver (a class field holding an array, a claimed-array local or parameter) now runs an inline plain-array tier — the same admission `js_array_pop_f64`'s fast path makes (`GC_TYPE_ARRAY`, not forwarded, no frozen/sealed/no-extend/descriptor flags, prototype latch clear, `0 < length <= capacity`, no hole) — with the runtime call kept as the fallback for everything else. In the wolf-ecs entity cycle the plain-array `packed.pop()` was 8–10% of self time on nothing but that fast path's call overhead.
