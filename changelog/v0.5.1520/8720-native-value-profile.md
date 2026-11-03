### Added

- Stabilized the native value profile with checked exact-width scalars,
  source-linked and nested POD layouts, and value-copy semantics across local
  assignments and ordinary function boundaries. Invalid or imprecise native
  crossings now fail explicitly instead of truncating or losing precision.
