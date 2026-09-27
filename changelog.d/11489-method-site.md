A method call on an ordinary object (`o.m(args)`) no longer goes through the
universal runtime dispatcher on every call. The call site memoizes one shape's
answer — the method's own slot (inline or spill), or, for a method inherited
through ordinary prototypes (`Object.create`, an ES5 constructor's
`prototype`), the method closure itself validated by the global
prototype-validity word — and calls the method body directly with the
receiver as `this`. Function-object receivers (`F.m()`) keep the dispatcher
until functions carry a shaped property record.

A write to an existing slot of an object used as a prototype now moves the
prototype-validity word, and the store caches never learn such an object's
shape, so reassigning, redefining or deleting an inherited method is seen at
the next call. The implicit-`this` cell is reached through a per-agent
pointer block (`PERRY_AGENT_PTRS`, initial-exec TLS on ELF executables), so
every emitted `this` save/restore there is a load and a store instead of two
runtime calls. Layout constants generated code bakes in live in a new
dependency-free `perry-abi` crate that the runtime asserts against.
