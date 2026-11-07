A function object's own data keys are now a key-add on its shape.

CreateDataProperty (class fields on callable class instances) and `[[Set]]`
(`Object.assign` onto functions, computed-key stores) on a function answer from
the layouts: the bag's own key position, the function's chain from the link its
shape names, the bag's published key-add edge, and the closure's next keyed
Function shape from its own ShapeId's key-add edge (validated per use). The
generic named-Set tail, the per-hop `getPrototypeOf` chain walk and the
re-definition of default attributes are gone from these paths. A function's
bag is born once (a bound function's `name` was defined from inside its own
bag birth) and, when born by a program put, with inline room. The layout probe
reads a bound function through its bag and shape link. Fixed: a constructor's
`prototype` materialized after other own keys (a read, `Object.freeze`, an
assignment) enumerated last instead of after `length` and `name`.
