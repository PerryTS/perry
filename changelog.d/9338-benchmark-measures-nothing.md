### Fixed

- Benchmark rows whose Perry median is `0 ms` are now rejected as
  `MEASURES-NOTHING` instead of being reported as impossible `0.00×` wins.
  The loop-overhead and string-concatenation fixtures also retain observable
  work and run above the internal timer's resolution floor.
