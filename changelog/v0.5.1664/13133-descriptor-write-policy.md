### Fix `node:zlib` property descriptors and writes

Legacy constant aliases now expose read-only, non-configurable own descriptors
while remaining non-enumerable. Writes to constants and protected module exports
are rejected before namespace overrides can change their values.

Descriptor values and ordinary reads share the same cached `constants` object.
The fully populated bidirectional `codes` object is now frozen before publication,
with its construction protected by runtime GC handles. Ordinary function exports
remain mutable and can be restored after an override.
