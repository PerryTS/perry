Fix non-spread native superclass method calls by sharing the accessor's actual-home-prototype lookup and receiver-aware Reflect.get. Remove the declared/relinked-chain latch, native-method stashes, and collection-specific super dispatch.

When linking a class prototype, the evaluated superclass's own prototype now precedes the declared-id fallback. This preserves native function-valued heritage even when a reserved class id names other runtime metadata. The regression materializes an fs.Stats prototype before forwarding Stream.on; Node and Bun return the emitter and deliver the event, while the base fails.

Review: resolve a function superclass's prototype through ordinary Get and materialize the existing child prototype at definition time. Codegen passes its rooted interned property key to the shared super-call operation, removing per-call string allocation. Transform.prototype now owns its specified default _transform hook. This PR is stacked on #12190's unified explicit-this operation.

Preserve an own function prototype data slot whose value is undefined, using the existing shape slot verdict, and root the receiver across prototype accessors. Invalid superclass prototypes now throw instead of being replaced by a lazy synthetic object.

Semantic rebase: retain main's single explicit-this forwarding mechanism.
Every fresh class evaluation performs one ordinary superclass.prototype Get
and validates it at definition time. Its rooted parent and prototype operands
are passed into creation of that evaluation's own prototype; computed member
names run after validation. This removes the template's cached prototype from
fresh evaluation validation and removes the later, narrower dynamic-property
read. Published allocation entry points retain their ABI; new entry points
carry the two operands and are marked Reenters in all call-effects tables.

Fresh evaluation metadata and prototype members are defined as own properties,
using the existing descriptor operation. Ordinary Set had spuriously consulted
the instance setter chain during birth and materialized the shared template
prototype, performing an extra observable superclass.prototype Get. Own
definition removes that lookup while preserving each property's attributes.
