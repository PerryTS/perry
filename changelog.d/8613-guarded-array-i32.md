### Performance

- Propagate runtime-backed numeric-array parameter guards into native
  representation selection, allowing Feistel-style accumulators seeded from a
  guarded `number[]` to stay in canonical i32 slots after a dominating bitwise
  normalization. Nested bitwise expressions now also consume mutable
  Number-by-construction locals and bounds-proven typed-array reads without
  falling back through `js_number_coerce`.

- On the exact `typed_array` workload from #8606, retired instructions fall
  from a 14.68B three-run median to 9.66B (-34.2%), cycles from 8.10B to 5.96B
  (-26.5%), and user CPU from 2.73s to 2.01s (-26.4%), with the checksum
  unchanged. The specialized `encipherUntyped` function now contains zero
  `js_number_coerce` calls.
