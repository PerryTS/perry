Counted numeric property-read loops now resolve their predicted ordered key list
through the canonical shape tree and compare the receiver's header once before
the loop. The body uses constant slot offsets. No receiver-derived IC word or
slot cache is created; a miss calls the generic property reader directly.

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
