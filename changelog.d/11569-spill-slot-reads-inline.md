Property reads of keys stored in an object's SPILL (overflow) storage — any key
added by name past the object's inline slots — are now served by the same
inline shape compare as inline keys: after the ShapeId match, two dependent
loads reach the spill buffer and one reads the value, instead of a call into
the runtime. The ShapeId alone proves where the value lives: a key claimed
without a value (`Object.defineProperty` with no `value`, accessor installs,
wholesale key lists) now gets spill storage, a stored `undefined` survives the
spill buffer growing, and no spill entry is published while spill storage is
disabled. A spill key holding `undefined` now primes its read site (it used to
take the by-name walk on every read), and a latched megamorphic site answers a
spill-located key from the receiver's shape.
