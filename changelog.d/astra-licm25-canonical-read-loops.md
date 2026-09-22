Counted numeric property-read loops resolve each predicted ordered key list once
at module initialization. A registered guard-shape global holds the expectation;
its external-carrier descriptor roots the keys even with no live receiver. The
preheader compares the receiver's header shape once, and the body uses constant
slot offsets. A miss runs the original loop lowering, including its existing
read optimizations. Calls observed only with short constant bounds keep that
lowering without a preheader, so frequent short misses pay no versioning cost.

Admission is a closed grammar: one numeric addition reduction, an unchanged
receiver, a primitive bound and counter, and no calls or existing-object stores.
Numeric prechecks exclude implicit coercion callbacks. A fresh array of primitive
literals/counter values may be assigned to a separate, otherwise unread binding;
that allocation cannot dispatch JavaScript or alter the receiver. Direct module
roots have the same proof as local roots. Captures and boxed bindings are refused.
Moving polls remain enabled, and each iteration derives the receiver address
again from its registered root.

Shape identity is not universally immutable: dictionary key lists and private
stable-tombstone epochs can change while retaining their IDs. The prediction
resolves an ordinary, generation-zero, hole-free, entirely inline canonical
shape, so neither exception can match it. At the base SHA dictionary restamping
still supplies `Ordinary`; exclusion depends on its distinct generation and
keyless descriptor, not that enum tag. GC relocates payloads and shape key edges
without changing the receiver's ShapeId or slot order.

Coverage includes emitted guard/constant-slot assertions, refusal of stores,
calls and receiver reassignment, canonical identity across construction paths,
dictionary exclusion, Node differential cases for getters/proxies/coercions and
non-objects, and an allocating moving-GC fixture in the root-dominance corpus.
