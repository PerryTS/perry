The unknown-receiver probe now answers a read that reaches an accessor from
the receiver's shape, at every named site and without a name gate: the pair
in the key's slot is the authority, for the key list, the overflow table and
the first prototype that holds the key. A getter whose compiled body is
exactly `return <captured binding>` carries that fact on its own
`JsFunctionInfo` (`FN_RETURNS_CAPTURE`: capture index and cell offset in the
trusted-clone words, which such a body does not use), so its read is the
binding load the call would execute, live bindings included; a binding still
in its dead zone, and every getter with any other body, is called exactly
once per read by the site's decline edge. A pair without a getter reads
`undefined`. The decline edge's layout walk now also serves accessors kept in
an overflow table, which previously fell to the by-name read. CommonJS export
getters (tsc/Babel `Object.defineProperty(exports, "x", { get })`, SWC/TS
`_export`, webpack `__webpack_require__.d`) become a probe hit plus a few
loads.

The program-wide accessor-name set (`ClassAccessorNames`) and the
`read_may_reach_getter` gate are removed: named read sites own no shape/slot
memo. The CommonJS wrapper no longer declares, in the born `exports` layout,
a name that only `Object.defineProperty` writes, so a literal-key accessor
export is claimed in the object's own key list with its attributes instead of
moving the object to its overflow table. No cache, memo or side table is
added.

Tests: runtime probe tests for the binding, dead-zone, no-getter, called
getter and overflow answers; a codegen info-render test; gap tests for CJS
export getters (literal and `_export`, live binding) and for accessor
semantics (effects called once per read, reassigned and dead-zone bindings,
overflow accessors and setters with the right `this`, class getters).
