Call compiled class getter and setter bodies from sites authorized by the live
receiver and complete prototype ShapeIds, including inherited accessors. Misses
retain ordinary property semantics for prototype replacement, instance
shadowing, primitive receivers and subclass overrides. The shared leaf guard
adds no cache, root holder or runtime registry.

Consume existing containment and proven-this capabilities at accessor calls,
and use the existing typed receiver lowering for pure numeric getters. Compact
receiver bodies enter the existing pre-statepoint inliner, removing call ABI work and
redundant field guards without property-name special cases. Collecting accessors
keep ordinary statepoints and relocation.

Regression coverage includes mutation, inheritance with lexical super and
virtual dispatch, representation changes through callbacks, a missing-proof
negative control, and moving collections in getter/setter bodies.

Discard unused private accessor capabilities before codegen-unit splitting
promotes their linkage. This prevents unused accessor receiver bodies from
adding executable text in packages such as Zod.
