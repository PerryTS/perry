Fix constructor property insertion order: assignments to `this` reserve compiler layout facts and storage without becoming class field definitions. Constructor, method, conditional, native-super and inherited stores now create keys through the same property-store shape transitions. Capacity reservation remains separate from key membership. This also removes synthetic `undefined` initialization before constructor assignments.

Coverage compares Object.keys, for-in and JSON.stringify with Node for method-added keys, EventEmitter subclasses, conditional stores, repeated stores, delete/re-add, subclass ordering and undefined values.

Named declared fields begin without own keys when their definitions still have to execute, so base constructor properties precede derived field definitions. Named, computed and arrow initializers use the existing DefineField helper; this removes the predeclared-own-property shortcut and preserves inherited-setter and replacement-receiver semantics. Literal materialization and complete, unobservable bare-field initialization retain fused allocation. Inline allocation uses the existing header image with a capacity-only shape, without new runtime state.

Runtime class-object, class-reference and newTarget construction share the same capacity-only allocator. Coverage includes capturing class expressions, repeated instances, separate class evaluations and Reflect.construct.

Field definitions share the existing shape-authoritative definition entry before adding fallback roots, and rely on the store funnels' existing receiver validation. Proved birth fills complete bare field definitions without a duplicate initialization phase. Constructor key-add publication reuses the store site's existing clear-chain verdict, removing a duplicate prototype walk without adding runtime state.

Interned key-adds with an existing chain proof retain the checked transition funnel while creating runtime handles only for interning, chain inspection or spill growth. Inline appends remove three unconditional handles; cached-stamp misses leave the receiver unchanged for the rooted fallback. Coverage requires live cached append edges and checks pointer values in inline and spill storage.

HIR field origins distinguish definitions from constructor-store reservations. Reservations retain existing field types, slot hints and guarded reads, but never run an initializer or fuse an assignment into birth. No runtime state or additional key-publication path is introduced. Specialization and stable hashing retain this semantic origin.

Closed literal constructors do not repeat the default definitions already completed by literal birth fills. The existing transition helper specializes its proved-chain, interned-key path at compile time, removing optional-root branches and a duplicate header validation while retaining the same checked stores and spill roots.

Cached ordinary appends still publish an `undefined` key, but omit an identical reserved-slot value write. Nonempty reserved slots are cleared. Other ordinary appends reuse the existing checked store funnel without repeating numeric-prefix proof retirement; representation checks, string-alias handling and GC barriers remain in that funnel.

Object-kind queries read the existing per-agent shape slab directly. The redundant hashed kind cache, its allocation and its separate publication/retirement work are removed, so deferred key transitions have one source for this shape fact too.

Exact cached ordinary appends retain the representation admission's canonical slot bits for the existing barrier funnel. This removes a duplicate representation check while retaining the original assignment result; reminted successors still use the full checked store. Coverage exercises tagged integers, negative zero and NaN on generic and proved-chain appends.

Constructor reservation stores use the existing static-key store IC in both assignment modes, so a runtime key order does not force every store through a failed declared-layout guard.
