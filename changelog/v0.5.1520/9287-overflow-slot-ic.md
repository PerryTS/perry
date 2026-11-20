### Performance

- **Property access past an object's first two slots is now cached.** A plain
  object keeps its first two properties in inline storage and the rest in an
  overflow buffer — and the constant-key inline caches refused to prime for
  the overflow region, so `obj["field_x"]` on any wider object missed its
  cache on every single access, forever. Measured at 27 ms against 3 ms for
  the identical loop, decided entirely by whether the property was added
  second or third. Overflow slots now prime with the same encoding the
  dynamic-key cache has always used, and hot access to a wide object's
  fields runs ~5× faster (27 → 5 ms; node: 2 ms).
