### Fixed

- A `new` on a named or default import of a package that is not installed is
  now a compile error naming the package and the binding, as a call of one has
  been since #11576. A call failed in the linker; a construct links, and builds
  a plain object whose every member reads `undefined`, so the build reported
  success over a program that cannot work. Node fails the same program at load
  with `ERR_MODULE_NOT_FOUND`. A property read or a type-only use of an
  unresolved binding is unchanged, since neither fabricates a value.
