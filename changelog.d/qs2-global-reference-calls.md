Unify global `instanceof` operands and optional `typeof` reads with ordinary shape-validated property sites. Global replacement is read from the live slot; deletion resolves the binding before evaluating a getter, so an undefined-returning getter runs once. Lexical bindings keep normal scope resolution. Native constructor checks consume the evaluated RHS instead of looking it up again through the writable global binding, and avoid repeating the heap-native prototype walk.

Explicit receiver calls use the value-call dispatcher's live-header validation and inspect the capture word before arrow metadata. String index coercion uses a saturating numeric conversion, removing the separate libm truncation without changing NaN, infinity, fractional or object coercion semantics. Function-property fallback also drops its obsolete reentrancy flag: intrinsic resolution no longer recurses into that fallback. No new cache, latch, property-name list or side table is introduced.

Parity coverage exercises warmed global replacement, lexical and parameter shadowing, deletion, present undefined, a getter that deletes its own binding, and string index conversion boundaries.

Binding reads share the ordinary site’s positive own-slot front, including spill storage, so present `undefined` remains a hit while an absent binding resolves on the cold path.

Dynamic intrinsic RHS values use their existing constructor identity dispatch before bound-wrapper, callable, unrelated native-export and writable-name probes; declared-class exits remain ahead of this dispatch.

Array constructor and inherited-property reads follow the existing intrinsic prototype root and canonical keys, preserving prototype mutations while removing repeated global constructor/prototype lookups and fresh key allocation.

Native constructor dispatch and prototype walking borrow the operands' existing handles instead of rooting the same values in nested adapters; prototype target and traversal cursor remain rooted across lazy parent materialization. Optional global references retain the shape and spill front in outlined modules. Per-kind typed-array constructor values use the existing view classifier, preserving Buffer's Uint8Array superclass and excluding ArrayBuffer/DataView.

Ordinary instance checks read the actual RHS prototype and then resolve terminal DEFAULT/NULL links from the live shape, avoiding prototype materialization and redundant native/subclass probes after a complete miss.

Primitive and terminal ordinary receivers now take their tag/shape proof before constructor classification. The existing function shape's own data-prototype slot lets a terminal proof complete without handle scopes; otherwise the rooted ordinary walk reads the evaluated RHS. Bound targets, own hooks, changed prototype links, native cells and shape-branded native receivers retain their existing dispatch. Invalid RHS values retain their errors. This removes intrinsic-body classification from checks whose receiver already has a complete ordinary answer.

Prototype walks refresh each handle once per callback-free region and reuse terminal proofs at intermediate links. Constructor identification removes the duplicated dedicated-body ladder.

Shared constructor bodies now receive their intrinsic declaration as an immutable scalar capture at birth. Classification removes the public `.name` read, global-object search and duplicate name recovery for handle constructors, so renaming a constructor or installing a name getter cannot change its identity or unexpectedly run JavaScript. This declaration input uses the existing closure storage and constructor metadata; it adds no cache or side table. The shape remains the authority for global binding reads. Dynamic native dispatch also skips the static prefix's already-completed hook and proxy checks.
