### Fixed

- `String.prototype.normalize` no longer holds a borrowed heap-string payload
  across its `form` argument's `ToString` coercion. The coercion is a
  collection point twice over — an object form runs user `toString` (whose
  loop back-edge polls run a moving minor), and an inline short-string form
  materializes onto the heap — so a young subject could be evacuated while
  `js_string_normalize` held a `&str` into its pre-move address, after which
  the normalization pass read retired from-space. The form is now coerced
  first, the subject is rooted across the coercion, and the payload is
  borrowed only from the post-collection address. The observable orderings are
  unchanged: `ToString` still runs before the form is validated, so a Symbol
  form throws `TypeError` rather than the invalid-form `RangeError`. (#8426)
