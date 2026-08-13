### Representation selection: propagate fresh return shapes across modules (#7170 R2)

Native compilation now carries proven fresh anonymous-record return shapes
through exact import bindings and re-exports. Consumer modules can keep those
results as `Ptr<Shape>` values and use guard-free fixed-offset field loads and
stores instead of boxing at the module boundary.

A final-HIR prepass establishes the facts before parallel codegen, and imported
return-shape bindings participate in object-cache keys. Named classes, indirect
calls, unknown externals, and modules with representation-selection barriers
remain fail-closed.
