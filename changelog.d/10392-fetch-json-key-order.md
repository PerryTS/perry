### Fixed

- `Request.json()` and `Response.json()` now preserve JSON document order for
  non-index object keys while retaining JavaScript's numeric-key ordering
  (#10392, PR #11049).
