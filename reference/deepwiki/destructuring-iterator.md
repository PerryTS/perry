# Destructuring Iterator Reference Findings

DeepWiki raw responses:

- `reference/deepwiki/destructuring-iterator-engine262.md`
- `reference/deepwiki/destructuring-iterator-boa.md`

Implementation notes used for the Perry change:

- Array assignment destructuring must evaluate the assignment reference before
  consuming the iterator value, then perform `PutValue` after the value/default
  is resolved.
- Object assignment destructuring evaluates the property name first, then the
  assignment reference, then `GetV(source, key)`, then `PutValue`.
- Array destructuring owns an iterator record. If a target assignment or default
  initializer completes abruptly while the iterator is not done, the iterator is
  closed before the original completion is propagated.
- `IteratorClose` calls `return` only for an open iterator. If `return` is
  missing, close is a no-op. If `return` is non-callable, throws, or returns a
  non-object, the close path is abrupt; on the current Test262 bucket, the
  original throw completion remains the observed error while the close side
  effect still occurs.
