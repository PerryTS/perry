### Fixed

- User-installed `return` methods on Map and Set iterator objects now run for
  direct calls and `IteratorClose` on `break`, escaping `throw`, function
  `return`, and partial destructuring instead of being bypassed by the native
  class-id dispatcher (#9098).
