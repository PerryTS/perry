### Fixed

- **Finished the plain `ToString` operand-rooting audit from #6949.** The
  remaining `String.prototype.split`, `RegExp.prototype.compile`, rebound
  `RegExp` constructor, and patched typed-array `toLocaleString` paths now keep
  their raw receivers and arguments in runtime handles across coercions,
  callbacks, and result allocations. Each path re-reads the post-collection
  address before using it, so an evacuating minor cannot leave the operation
  reading or writing a forwarding stub.

- **Closed adjacent allocation windows in string splitting and RegExp argument
  coercion.** `js_string_split_n` now roots its source before allocating its
  result array (rather than after), and a rebound RegExp's flags value remains
  rooted while its pattern is coerced.

  The distinct raw-JSValues-in-Rust-containers family noted by #6949 is tracked
  separately in #7949.
