### Fixed

- Web builtin prototype methods on `URL`, `AbortController`, `AbortSignal`,
  `EventTarget`, `Event`, and `CustomEvent` are available as callable values.
  `URL.prototype.toString.call(url)` now returns the URL href, and inherited
  methods resolve through the `AbortSignal` and `CustomEvent` prototype chains.

  Statically-dispatched instance calls only ever covered `url.toString()` in
  call position. Every other way JS can reach an operation —
  `Object.keys(URL.prototype)`, `'toString' in url`,
  `const { abort } = controller`, `URL.prototype.toString.call(other)`,
  `for…in` — reads the property, so the operations have to be real own data
  properties. Same defect class as #10310; #10759 had already converted
  `URLSearchParams` for it.

  These installs are complementary to #10555's, not a replacement: WebIDL
  reifies an interface's *attributes* as accessor properties and its
  *operations* as data properties holding a function, and Node carries both
  on the same prototype at once. `URL.prototype` keeps its twelve component
  accessors and its `Symbol.toStringTag`, and gains `toString`/`toJSON`, in
  Node's own key order.

  Enumerability follows WebIDL, which differs from ECMAScript: these
  operations are **enumerable** (`Array.prototype.map` is not). The one
  exception is `AbortSignal.prototype.throwIfAborted`, which Node makes
  non-enumerable alongside its `reason` accessor; it is installed without the
  enumerability override, and the site says so. A new gap test
  (`test_gap_11003_web_proto_descriptors.ts`) pins the whole descriptor shape
  — kind, enumerable, configurable, writable, plus an independent
  `Object.keys` read — against the Node oracle, so the split is asserted
  rather than rediscovered.
