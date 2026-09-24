Fix anonymous class expressions resolving references to their enclosing local variable as a shared class template (#11153).

Only an explicit class-expression identifier creates an inner self-binding. Anonymous expressions now retain the enclosing local during member lowering and capture analysis, so repeated factory calls read their own statics and write their own prototypes. When a local receives a second anonymous class, lowering also invalidates the previous class identity recorded for that specific local. Mutable outer bindings continue to reflect reassignment; named expressions retain their separate self-binding.

Regression coverage includes factory statics, constructor identity, static and instance prototype writes, mutable outer bindings, replacement by a second anonymous class, and a named-expression control.
