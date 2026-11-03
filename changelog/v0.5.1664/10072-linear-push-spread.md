### Fixed

- Made repeated `array.push(...chunk)` scale linearly by updating array layout
  metadata and write barriers only for newly appended slots. A one-million-item
  fixed-chunk benchmark that previously timed out after 60 seconds now completes
  in 41.6 ms, while preserving iterator overrides, sparse-array semantics,
  self-aliasing, generic receivers, and old-to-young references under moving GC.
