An async function whose awaited promise rejects inside `try { … } finally { … }`
now runs the `finally` before the rejection reaches the caller. The rejection
resume uses the same throw routing as a generator's `.throw()`: the innermost
enclosing catch or finally (by nesting order, since nested tries can share a
start state) handles it, and the non-yielding finallys the completion leaves on
the way run first. This also fixes generators, which skipped an inner `finally`
when an outer `catch` caught the throw. A throw in a try's first state now runs
that try's `finally` when nothing catches it.
