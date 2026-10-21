A method call on an Error instance of a per-evaluation class (a class
expression from a factory, or a function-nested `class extends Error`) runs the
method found on the receiver's prototype chain. The dispatch tower's class arms
key on ordinary objects, so `e.toObject()` on opencode's `NamedError.create(...)`
errors returned an empty object, and every session error failed its schema. A
missing method on an Error receiver throws the TypeError node throws instead of
returning a placeholder object.
