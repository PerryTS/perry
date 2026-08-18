### Fixed

- `Uint8Array.prototype.map` and `toSorted` now keep their newly allocated
  output Buffer live while a user callback runs. A callback-triggered full
  mark-sweep could otherwise reclaim the output because its address existed
  only in a Rust local, producing corrupt results in the GC representation
  matrix when generational collection or write barriers were disabled.
- The compiled-package ambient `require()` regression test now expects the
  Node-compatible `MODULE_NOT_FOUND` code that the runtime intentionally
  returns for unresolved modules.
- The compiler-output gate now scopes the data-dependent numeric loop to its
  array merge blocks. Its previous generated-label prefix also selected the
  preceding setup loop and rejected that loop's legitimate one-time integer
  conversion.
