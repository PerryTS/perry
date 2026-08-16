### feat(codegen/runtime): stamp canonical class ShapeIds at birth

Compiled class instances now receive the stable ShapeId of their canonical
keys array during allocation instead of waiting for the first by-name property
lookup to stamp them. Module initialization mints one id beside each rooted
`@perry_class_keys_*` global; both the inline bump allocator and the outlined
allocator load that scalar and install it in the header's shape word before the
object is published. Scalar-replaced receivers use the same path when they
materialize.

This is #6759 C3 rung 2 and the next prerequisite for #7916's remaining header
shrink: rung 1 made the shape word valid for class instances, while this rung
makes it present from birth so a guard can depend on it without a lazy-stamp
window. The allocation-time parent id remains only as the ShapeId-exhaustion
fallback; inheritance already comes from the class registry after #7981.

The object representation is deliberately unchanged in this rung: a two-field
literal remains **56 bytes** and `retain` remains **168 MB written for 48 MB of
numeric payload** (3.5x amplification). The next guard-migration/header-layout
rungs consume the invariant established here; this PR does not claim a memory
or wall-clock improvement by itself. Per-object storage added: **0 bytes**.

Regression coverage proves both allocation forms consume the id minted at
module init, and a runtime test reads a fresh class instance's shape word before
any by-name access so lazy self-healing cannot make the test pass vacuously.

Refs #6759 and #7916.
