`Object.preventExtensions`, `Object.seal`, and `Object.freeze` now publish a new
ShapeId when they change an ordinary object's integrity flags. Previously,
`preventExtensions` left the shape unchanged, so a future shape-keyed property
add cache could reuse an edge learned while the object was extensible. Seal and
freeze only changed the shape indirectly when they updated an existing key's
descriptor, leaving keyless objects with the same gap.

Repeated calls with no new flag changes preserve the current ShapeId. A runtime
test covers all three operations on keyless objects and verifies that sibling
objects retain their original shape.
