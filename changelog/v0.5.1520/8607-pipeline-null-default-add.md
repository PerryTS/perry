### Performance

- Speed up generic registries and record-processing pipelines by caching stable
  array fields on proven-contained receivers and using a guarded numeric fast
  path for null-defaulted counters, while preserving dynamic JavaScript
  semantics on aliased and non-number fallback paths.
