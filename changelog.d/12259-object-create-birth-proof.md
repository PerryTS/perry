Remove the duplicate exact shape-interner lookup in `Object.create(P)` after
birth-width tracking settles at the allocator floor. Carry the canonical
zero-live birth ShapeId with its chosen allocation width into the existing
final-birth publication path; use the sole exact interner when a different
live bound or retired record requires resolution.

Keep prototype-word refresh, post-allocation shape revalidation, rooted
reminting, external-edge shading and carrier publication intact. Tracking and
learned widths still publish their actual live bounds. No cache or separate
allocation path is added. This change is limited to keyless Object.create
births; descriptor-copy and destination construction unification remain
separate work.
