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
