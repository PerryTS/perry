### Fixed

- GC slot rewrites now re-arm the closure side-table young log before
  publishing a rewritten dynamic property or static prototype, preventing
  minor collections from skipping newly young references.
