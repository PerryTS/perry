### Fixed

- **`node:stream` subclass overrides (`_transform`/`_write`/`_read`) are no
  longer ignored when the heritage reaching `class X extends <base>` is a
  local alias, an indirect subclass, a class expression, or a CJS
  destructured `require('stream')` — the shape nodemailer uses in every
  stream class it defines. `write()`/`push()` used to throw
  `ERR_METHOD_NOT_IMPLEMENTED` because the override was never installed on
  `this`; the dynamic `super()` dispatch now recognizes the resolved
  bound-export value regardless of how the heritage expression reached it.
