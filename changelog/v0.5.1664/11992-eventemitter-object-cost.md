EventEmitter instances use ordinary, traced JavaScript objects with shared
prototype methods. Shape-validated property access avoids repeated generic
lookups, and emit dispatch recognizes the resolved callable body, including
aliases, without testing the method name.

Deleting the last canonical key reinstalls the exact recorded parent shape;
deleting a non-last key still forks the private tombstone list. Reverse edges
live on shape records, and cached uses reject incompatible prototypes,
attributes and surviving field representations. Allocation and property-site
memos store scalar identities and validate them on every use.

Array-subclass numeric-tail transitions are relearned on ordinary append-cache
hits after full-trace carrier recomputation. This fixes the missing carrier bits
when a generic pop is followed by a cached push. Newborn emitter construction
also resolves prototype identity before allocation and uses the validated
newborn slot-store funnel.

Refs #10508, #11919.
