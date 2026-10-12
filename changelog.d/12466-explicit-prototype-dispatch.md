Fix class method calls ignoring `Object.setPrototypeOf` on an instance (#12466).
When a dynamic class dispatch site's receiver misses its canonical ShapeId and
has no own method, reuse the existing generic method operation to resolve the
live prototype. Canonical shape hits and own-method dispatch remain unchanged.
Regression coverage checks the fallback IR, explicit prototypes, own and
prototype mutations, changed data shapes, inheritance, `super`, and controls.
