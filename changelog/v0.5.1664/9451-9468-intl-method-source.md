### Fixed

- **Class methods and accessors now retain their original source text.**
  `String(C.prototype.method)`, direct `.toString()`, template coercion, and
  reflected getter/setter functions return the source MethodDefinition rather
  than a synthesized native-function body. Object-literal accessors also
  receive their specified `get name` / `set name` function names. CommonJS
  class expressions keep assignment-inferred names without exposing Perry's
  internal anonymous-default registration key. Fixes #9468.

- **Default `Intl.DateTimeFormat` dates now use the locale's CLDR numeric
  pattern.** The implicit numeric year/month/day field set, its
  `formatToParts()` output, and `Date.prototype.toLocaleDateString()` now agree
  on locale order, separators, and padding instead of falling back to the
  hard-coded US layout. Fixes #9451.
