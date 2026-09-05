### Fixed

- Isolate runtime test fixtures that inspect loaded libraries, process-wide box counters, and the composed symbol cache. Prevent neighboring tests from corrupting their assertions, and tighten the box reuse bound (#9197).
