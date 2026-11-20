Object.freeze, Object.seal and Object.preventExtensions now publish immutable
integrity facts on ordinary shapes and reuse a weak successor edge owned by the
predecessor shape. Reuse resolves the live ShapeIds, validates the successor's
facts and uses the existing cached shape-transition installer. A shared shape
therefore pays for its descriptor/key-list rebuild and shape publication once,
rather than once per object. Integrity transitions preserve field representation,
prototype, private brands and ConstFn facts.

Array descriptor routing tests integrity on the header it already resolved;
arrays without integrity flags skip virtual attributes, with no second header
resolver or GC-header probe on ordinary writes.

Dense array elements and length report integrity attributes through the existing
descriptor authority, avoiding one synthesized property descriptor per element.
Existing customized indices, named properties and symbols use the same ordinary
holder transition. Array preventExtensions and function integrity operations
publish their facts through that holder too, preserving main's named-store and
Function shape authorities. Sealed/non-extensible array holes reject later creation.
Empty typed-array holders use the shared transition; non-empty typed arrays reject
freezing and accept sealing, as required by the pinned Node oracle. Proxy integrity
loops root own-key arrays, individual keys and returned descriptors across traps.

The transition has no global cache or GC-pointer edge. Stale/pruned targets
decline; mutable private dictionary lists retain their existing descriptor path.
Runtime guard tests and Node-comparison fixtures cover shared successors,
representation, integrity levels, arrays, symbols, accessors, proxies and copying
GC. Integrity identity lives in the existing cold shape extension behind one packed
presence bit. The ordinary 72-byte record and slab stride stay unchanged;
ordinary descriptor hashes remain unchanged when those facts are zero.
