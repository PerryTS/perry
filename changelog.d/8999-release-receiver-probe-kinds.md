### Fixed

- Dynamic indexed reads now inspect `ObjectMeta.elements` only for receivers whose GC kind is an object. Array, string, Error, and other layouts can no longer be interpreted as an object metadata pointer, eliminating the release parity crashes introduced with Array-subclass elements storage.
- Managed heap cells are classified as closures only when their authoritative GC kind is `GC_TYPE_CLOSURE`. Reused `Error` storage whose padding retained the closure magic marker no longer loses `.message` or custom fields during property lookup.
- Removed the stale Linux parity allowance for `test_class_field_layout`, which now matches Node, and recorded the timezone-provider dependency added to the runtime in `Cargo.lock`.
