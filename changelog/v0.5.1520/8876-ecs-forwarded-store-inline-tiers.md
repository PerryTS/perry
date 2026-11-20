Array and ECS performance. Guarded property-receiver element stores follow one
growth-forwarding edge inline (the read tier already did), so a field that
kept a pre-grow forwarding stub (`this.ents[id] = arch`) no longer pays the
out-of-line extend helper and allocator resolver on every store; the raw-f64
downgrade note is gated on the header word already loaded. `typeof x ===
"number"` decides the definitely-Number cases inline, deferring INT32/class
refs, raw typed-array pointers and the Web Streams id band to the classifier.
Inline dynamic typed-array reads brand off the `GC_TYPE_TYPED_ARRAY` header
instead of the evictable 64-slot kind cache. `js_array_get_f64` and the
typed-feedback array-read fallback route object-backed Array-subclass
receivers to their dense fast read before the tracked resolver and the by-name
key path, and an `Any`-typed key that is an integer array index takes the
inline numeric read tiers (`a[b[i]]`). Object-backed Array subclasses keep
validated prototype-override reads, and compact guarded specializations are
pre-statepoint inlined by lowered IR size.

wolf-ecs (noctjs/ecs-benchmark) on the Mac mini reference box, versus the
previous retained build: add/remove -18.5% (0.556 → 0.453 ms/op), entity-cycle
-23.1% (0.499 → 0.384 ms/op); each step 11/11 paired wins, semantics probes
byte-identical to Node.
