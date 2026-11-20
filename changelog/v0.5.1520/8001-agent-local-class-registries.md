### Fixed

- **Class prototype and dynamic-value registries are isolated per `perry/thread`
  agent (#8001).** Tables keyed by process-wide codegen class ids previously
  stored raw pointers, NaN-boxed values, closure identities, or realm-created
  Symbol keys in process-global maps. The first agent to materialize a class
  prototype could therefore make every later agent link instances to its heap,
  and any agent's moving collector could rewrite entries owned by another
  agent. Pointer-bearing class side tables now use Perry's hot thread-local
  storage, while code-only metadata such as vtables, static method pointers,
  bind lengths, and registered class ids remains process-global. A two-live-agent
  runtime test and a `perry/thread` behavioral probe pin distinct, non-null
  `Class.prototype` identities and realm-local prototype mutations.
