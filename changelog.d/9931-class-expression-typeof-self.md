### Fixed

- Resolve a named class expression's inner name inside `typeof`, including
  when its outer binding has a different name and the compiler registers the
  class under a generated key.
