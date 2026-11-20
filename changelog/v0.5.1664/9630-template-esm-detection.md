### Fixed

- **Ambiguous `.js` files now recognize module exports after complex template
  interpolations (#9608).** Perry now uses SWC's program parser to find real
  top-level module items instead of maintaining a partial byte scanner, so
  nested templates, escaped backticks, regex character classes, comments,
  strings, and division no longer hide a trailing export. Genuine CommonJS
  inputs retain sloppy Script semantics.
