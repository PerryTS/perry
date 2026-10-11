Object literal spreads over ordinary data shapes now birth their combined
result shape once and copy slots directly. The existing transition authority
keys this edge by the source and literal-prefix shapes, with its existing weak
GC ownership, instead of replaying property definitions and shape transitions
for every key. Dictionary, proxy, accessor, symbol, and hidden-property sources
retain CopyDataProperties. Each literal spread emits one shared runtime call
and keeps the existing literal-definition semantics for static parts. Leading
and prefixed literals have two fixed ABI entrances sharing the same generic
fallback and transition authority; no helper or guard is emitted per site.

Static-key spread literals lower directly through the source-ordered HIR
builder, removing their synthetic closure allocation and call. Home-object
methods, computed keys, accessors and prototype setters retain their complete
source-ordered lowering.

Leading spreads preserve shape-proven Number lanes and bulk-copy them without
pointer barriers. A source representation or descriptor change retires the
source shape and its admission. Other data slots use the existing barrier
funnel. Nursery bump refusals root the source, prefix and target key list
before the normal collecting allocator runs. Cached shape births therefore
retain collection pacing rather than reserving blocks under suppression.
Empty leading sources use the normal keyless allocator as well.

Literal own-slot admission stays inside the shared runtime entry; numeric
stores share their slow funnel with named Set. Transition records are not
spilled merely for a cold allocation refusal. Scalar shape ids use all their
low bits in the existing aligned-key hash.

Class fields, custom prototypes and keyless construction retain key-add-first
admission. Region priming borrows one authoritative slab record instead of
lifting the descriptor twice, with the same key and inline bounds.
