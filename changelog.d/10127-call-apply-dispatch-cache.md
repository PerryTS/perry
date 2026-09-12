### Performance

- **`Function.prototype.call`, `Function.prototype.apply`, and direct spread
  calls no longer hash-probe the closure-body registry on every invocation.**
  Their rest-parameter check now shares the existing four-entry dispatch memo,
  retaining late-registration invalidation and all 0–16 argument, receiver,
  bound-function, rest-array, and imported-class semantics. Refs #10085.
