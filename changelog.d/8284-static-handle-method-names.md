### fix(runtime): keep bound handle method names alive across GC

Computed method-value reads on node:sqlite, TLS, EventEmitter, and
AsyncLocalStorage handles now bind static method-name literals instead of
retaining pointers into movable GC heap strings. Primitive-number method reads
use the same static-name rule, so closures such as `(5).toString` cannot retain
the computed key's interior after collection. Fixes #8178.
