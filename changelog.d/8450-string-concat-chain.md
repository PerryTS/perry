### Performance

- Fuse `String.prototype.concat` calls of up to 31 arguments into one
  `js_string_concat_chain` allocation instead of allocating and copying the
  growing prefix once per argument. An 8-argument call now emits one chain
  helper instead of eight pairwise helpers; the 6,000-iteration growing-string
  fixture measured 0.18 s median on macOS arm64.

### Fixed

- Preserve JavaScript's argument-evaluation-before-coercion order for
  `String.prototype.concat`, and keep the receiver, raw arguments, and coerced
  strings rooted across re-entrant user `toString` calls.
