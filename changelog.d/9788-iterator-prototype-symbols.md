Keep class iterator methods symbol-only in prototype own-key enumeration, and
observe replacements of Symbol.iterator during direct calls, spread, Array.from,
and for-of loops. Prototype accessors receive the instance and run once; an
explicit undefined replacement shadows the original class method.
