### Fixed

- Native instances assigned with `target = new NativeClass(...)` or propagated
  with `target = source` are now tracked by the resolved binding rather than by
  identifier text across the whole module. A native handle named `O` can no
  longer make unrelated bindings named `O` dispatch ordinary methods through
  that native class, while module-level handles and unresolved global fallbacks
  retain their existing cross-function behavior.
