Fixed inherited `name` on Effect tagged errors (#10890).

A factory-created `Base.prototype.name` could be lost when a class expression
or function-local class declaration used a shared template parent instead of
its evaluated parent. Nested `super()` replay also replaced the instance's
class pin with a deeper Error ancestor, so property reads found `Error` before
the tag. Perry now retains the evaluated heritage and first constructor pin,
then reads inherited properties from that evaluation's prototype chain.

The parity fixture covers distinct tags, `String(error)`, and Effect's nested
`Data.Error` inheritance shape. The pinned Effect package repro now matches
Node for `_tag`, `name`, `instanceof`, the declared field, and `String(error)`.

Also fixed `util.inspect` printing perry's hidden runtime-internal own keys.
They physically live in an object's `keys_array` but are not JS properties, and
every other own-key consumer (`Object.keys`, `for…in`, `getOwnPropertyNames`,
`JSON.stringify`, `hasOwnProperty`, spread) already filters them through
`is_internal_runtime_key`. Inspection was the lone hold-out, so any instance of
a per-evaluation class object rendered its #10624 constructing-class pin —
`Error: boom { __perry_ctor_class_object: … { __perry_parent_class: … } }` —
where Node prints just `Error: boom`. The leak pre-dates this PR (it was
already visible for a class expression carrying a static field); routing
heritage-carrying class expressions through the fresh-class path brought
`test_gap_9440_error_name_ownership`'s `escaped-dynamic` case onto it, which is
how it surfaced. `showHidden` deliberately still does not reveal these keys: it
exposes non-enumerable JS properties, and these are runtime bookkeeping.
Pinned by `builtins::formatting::internal_key_hiding_tests` (three cases, each
asserting its key really is in the allowlist before asserting it is hidden, so
a renamed constant fails the precondition instead of passing vacuously).
