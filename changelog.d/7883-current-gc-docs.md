### GC documentation now has one current source of truth and rejects deleted knobs

The generational collector accumulated several chronological plans whose opening
decisions no longer matched the shipped implementation. A deleted
`PERRY_GEN_GC_EVACUATE` setting also remained in required memory-stability arms,
ratchet metadata, current documentation, and the generated translation catalogs;
those test arms looked distinct while selecting the same runtime behavior.

A dated collector architecture/operations page now records the shipped collection
paths, target-specific root lowering, barriers and weak processing, pressure and
pooling behavior, supported controls, old-generation defragmentation status, and
the CI contexts that are actually required. The experiment journals are explicitly
historical, and stale source paths, checker status, engine-plan authority, and gate
freshness cadence are corrected.

The dead setting is removed from every live/generated claim. A new CI audit derives
accepted GC knob names from uncommented production runtime, codegen, and compiler
parsers, while allowing only three path-exact historical journals. Its self-test
plants a deleted knob behind a commented-out parser and proves that neither can make
a live claim pass.
