Class getters and setters resolve through the accessor lanes of their current prototype holders (class-table retirement S4, Refs #10502). Generic reads and writes follow the instance's actual chain, and deep getter/setter sites check every intermediate ShapeId as well as the holder ShapeId and pair. Prototype replacement, nearer shadowing properties and relinks therefore invalidate the recorded answer.

Remove the class-table accessor predicates, the class-id store-verdict registry and its vtable-generation dependency, the duplicate callable-getter dispatch arm and the obsolete prototype-getter bypass. The typed-field guards trust own data properties, which shadow inherited accessors. Keep the declared-getter-name gate on the emitted accessor arm and the existing plain-closure/builtin getter ABI from #11925.

Coverage includes a getter replaced with Object.defineProperty, a getter inherited through three levels, a setter on a subclass instance, intermediate shadowing, relinking and a newly installed accessor. Runtime tests prove that intermediate ShapeId changes reject primed deep getters and setters, and that a late setter install revokes a negative store verdict.

Fetch prototype getter lanes now run native property bodies, including on Request/Response subclasses. Native EventEmitterAsyncResource construction seeds its own accessor descriptors through an own-property definition, preserving inherited getter-only properties.

ClassBody method installation defines own properties directly, so an inherited setter cannot intercept a definition. The accessor walk declines the general prototype builder's temporary self link. Negative walks skip accessor-free holder shapes and check data shadowing only after finding an accessor; misses allocate no handle scopes.
