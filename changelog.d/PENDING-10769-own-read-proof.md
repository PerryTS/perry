Own data-property reads preserve receiver and value proofs through their
consumers. Scalar-replaced fields reuse the existing containment and
whole-write Number proof, avoiding redundant numeric coercion, string alias
checks and collection polls in numeric scalar loops. Branches that can store
a nonnumber retain the generic path.

Runtime named reads preserve canonical key identity and consult the receiver's
live shape for small, plain inline data slots before the generic read walk.
Descriptor mutations, private fields, indexed or exotic receivers, wide key
lists and spills retain their existing fallback. The positive lookup adds no
runtime registry or cache and does not allocate or invoke user code. A live
nonordinary shape also retires the data lane immediately, preserving its
generic caller's class and special-receiver handling.
Wide, semantic and absent-key reads reuse the first live shape's layout and
slot bounds in the existing fallback walk, avoiding a second arena/type and
descriptor lookup. Inherited hops and unproved cells retain their checks.
Owner words outside the ordinary shape namespace decline the data lane before
hashing the key; their existing generic callers retain receiver semantics.

Guarded loop regions retain an emitted-body collection-effect proof through
the back-edge decision. A split loop omits its poll only when every admitted
inline/spill copy is verified unable to collect, no generic body is reachable,
and its controls are independently proven noncollecting. Generic fallback
loops, unknown calls, collecting bounds and rechecking regions keep their
polls. The proof adds no runtime state.

Receiver-only loop regions admit an invariant plain local bound with one
strict Number entry test, using the existing guarded Number scope. Boxed or
captured bindings, mapped arguments and bounds written by the loop are
excluded; nonnumber bounds retain their coercions in the generic loop.
