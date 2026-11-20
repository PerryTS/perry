Initialize the cycle partner of a module that is reached only through a dynamic
`import()`. Perry drops init-call back-edges from a module's `__init` wrapper so
that a cycle member the entry's eager init loop already ran is not re-entered
early, but that loop skips Deferred modules, so dropping the edge to one left it
with no caller at all: its body never ran and every export it assigns at run time
stayed undefined, surfacing as `TypeError: value is not a function` on the first
call through such a binding. The positional drop now applies only to Eager deps.
An Eager module's static imports are themselves statically reachable from the
entry and so are Eager, which is why the ordering this rule protects is
unaffected, and the existing per-module init guard keeps the extra call
idempotent and cycle-safe.
