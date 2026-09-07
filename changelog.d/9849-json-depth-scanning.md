Use bounded ARM64 block scans for quoted spans in the JSON nesting preflight.
Long ordinary spans use a separate bulk scanner, and long escaped spans
classify complete backslash runs with integer masks. The preflight remains
allocation-free and preserves its handling of malformed input; syntax
validation, non-ARM scanning, construction batching and GC policy are unchanged.

Add exhaustive backslash-run/carry checks, scalar depth-oracle comparisons
and protected-boundary tests for the new loads. Performance qualification is
tracked separately from this experimental implementation.
