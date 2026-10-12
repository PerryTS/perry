Property updates reuse the ordinary store site's ShapeId/slot memo for a single
own-data-slot read, numeric operation and write. The shared leaf replaces the
per-site by-name read, ToNumeric, numeric step and generic write sequence; one
shared generic continuation preserves accessor order, strict writes, strings
and BigInt results. Inert numeric compound and logical assignments use the same
memo; effectful right-hand sides retain the ordinary sites and recheck after
user code. Computed updates use the existing indexed read and dynamic store
sites, with once-only evaluation of the receiver and key expressions.

Compound snapshots retain the class hint for `this`. Gap fixtures cover all update forms, compound operators,
short-circuiting, getter/setter order, changing representations and shape
mutation during a right-hand side.

On baseline 3c2f9b065a the supplied recursive Arena benchmark's left/right
reads already pass the inline class shape guard. The observed by-name read in
the Arena micro is the `size` update; the read guard needs no speculative change.
