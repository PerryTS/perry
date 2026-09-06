Speed up JSON parsing and stringify with bounded NEON/SSE2/word string scanning, direct Unicode escape emission, and a temporary key index for very wide objects. Preserve strict syntax validation, duplicate-key order and existing GC boundaries; prove depth one cheaply for large flat scalar arrays.

Use stack-buffer ECMAScript number formatting in JSON serialization, removing per-number temporary allocations and fixing shortest-digit tie rounding. Numeric-array stringify improves a further 20%; 1,050,129 exact double values match Node and Bun.

On a quiet Apple M1, a fresh-runtime A/B measured 37.5× faster parsing of a 50,000-key object, 7.1× faster parsing and 4.7× faster stringify of a 1 MiB ASCII string, and 1.44× faster numeric-array parsing. Tiny JSON is essentially unchanged. Wide-object retained RSS increased by roughly 2 MiB. The reproducible harness, full Node/Bun comparison and validation records are in `benchmarks/json_performance/`.
