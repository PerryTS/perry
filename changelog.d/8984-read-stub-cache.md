A megamorphic stub cache for dynamic string-keyed property reads — the read
twin of #8965/#8977's write stub, 2-way set-associative from the start.

A hit skips `js_object_get_field_by_name`'s fast-lane guard chain (address
class, interned-key flag, arena classification, header type/flags/class,
keys-array validation) and the read-plan probe. That probe matters more than
its own cost suggests: the plan's epoch is bumped by the incremental collector
at loop-poll cadence, so on a steady read loop it is repeatedly cold and falls
through to a shape-index hash lookup.

Interleaved A/B, min-of-21: pure property read 17 → 15 ms (−12% min, −17%
mean), computed-key read 23 → 22 ms, combined overwrite 41 → 39 ms, write
unchanged.

**Safety.** Entries store CONTENT bits, never an address, so a key that dies
and has its address recycled cannot produce a false hit; keys that do not fit
the inline form are not cached. Every hit re-validates heap-object type,
not-forwarded, blocking flags, class id, and the receiver's CURRENT shape
token — which pins the exact key set *and order*, so a match means the cached
slot still names this key. The probe sits after the `process.env` and Proxy
arms, which keep their own semantics, and the stub is only primed from inside
the lane, once the receiver is proved ordinary.

Because a wrong-slot read would be silent corruption rather than a crash, this
carries an adversarial differential: a delete that changes the shape under a
cached slot, an accessor defined over a cached data slot, prototype fallback
after the own property is deleted, `Object.freeze`, the same two keys inserted
in opposite orders, and a 300-key object whose slots live in the overflow
store. Output is byte-identical to node on all of it. Suite 2779 passed;
private-member output identical to base.
