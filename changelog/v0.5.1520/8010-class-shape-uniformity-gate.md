### Birth-stamp the class allocators #8009 left lazy, and gate the split population (#7983)

#8009 (C3 rung 2) stamps a class instance's ShapeId at birth on the **compiled**
path. Three other class-instance allocators were left on rung 1's lazy
self-heal — `js_object_alloc_class_with_keys`,
`js_object_alloc_class_dynamic_parent`, and the `js_object_alloc_class_inline_keys`
compatibility entry point — so for any class reaching one of them the shape's
population is still split between stamped and newborn receivers.

That is not a slow start. The emitted read PIC derives its entire cache token
from the header shape word, so a stamped receiver and a newborn one of the same
shape compute two different tokens and the site's hit rate is **0% forever**:
instance #1 misses, is stamped, primes the id token; instance #2 is newborn,
computes the keys pointer, misses; the handler re-primes the same id; instance
#3 misses.

This is the defect bisected to `4784d5da7` (#7983). On instructions retired,
isolated against its own parent: `cycles` +54.3%, `deeplist` +45.2%, `interp`
+28.3%, `pipeline` +23.9%, `iso_miss` +22.9% — while the object-literal
benchmarks (`churn` +1.2%, `retain` +0.2%) and `fib40` (+0.04%) did not move,
literals having been birth-stamped since #6804. Isolated further on one program
and one build: a read pass over 3,000,000 newborn class instances costs 43.6
instructions per read, a second pass over the same now-stamped instances 15.5.

All three allocators now stamp at birth. The two shape-cached ones read the id
out of the `ShapeCacheEntry::runtime_shape_id` their existing probe already
returns; the compatibility entry point mints from its canonical keys array, off
the compiled hot path.

The gate, `a_fresh_class_instance_computes_the_token_the_miss_handler_primed`,
asserts the token the miss handler PRIMES equals the token a freshly-allocated
sibling COMPUTES, comparing against the emitted IR's formula transcribed into
the test. It fails on `main` as of #8009 and passes here. "A newborn carries a
stamp" is a presence check that both-stamped and both-unstamped each satisfy —
only the mixture is the bug, and only a test comparing the two sides can see it.
This one passes in either uniform state, so it survives a future policy flip.

Working notes, including the full bisect and the two dead ends: `gc-handoff/BISECT-NOTES.md`.
