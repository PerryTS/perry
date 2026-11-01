### Fixed

- A `new` on a named or default import of a package that is not installed is
  now a compile error naming the package and the binding, as a call of one has
  been since #11576. Construction was excluded then on the grounds that it
  "links, and throws where it is reached": it does link, but nothing throws —
  the constructed value is a plain object whose every member reads `undefined`,
  so the program ran to completion with fabricated results and exited 0, with
  no failure to trace back to the missing package. Node fails the same program
  at load with `ERR_MODULE_NOT_FOUND`. A property read or a type-only use of an
  unresolved binding is unchanged, since neither fabricates a value.
