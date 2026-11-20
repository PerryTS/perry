### Fixed

- **Sloppy `caller` and `arguments` writes on ordinary non-strict functions
  now match Node.** Perry previously threw unconditionally when these inherited
  poison-pill properties rejected an assignment. The compiler now preserves
  the function kind through runtime registration, allowing sloppy writes to
  remain silent without creating an own property while strict functions,
  classes, methods, arrows, async functions, and generators keep throwing.
