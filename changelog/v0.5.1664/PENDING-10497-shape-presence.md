Ordinary string-key `in`/`[[HasProperty]]` queries walk each object's current shape keys and prototype edge without reading property values, allocating names, or invoking getters. Accessors and inherited data properties containing `undefined` remain present. Dictionary receivers and prototypes, exotic receivers, and class virtual surfaces retain generic dispatch; dictionary presence uses the object-owned key list through deletions and re-additions.

Null-parent objects, including `Object.prototype`, publish the null edge in their birth shape. Prototype fallback distinguishes an explicit null before `Object.prototype` from the intrinsic's own null parent. `Object.getPrototypeOf` uses the existing owner/shape default-link proof before iterator-exposure probes.

Installed builtin prototype methods record their non-constructor capability on the function body. `call` and `apply` inspect the callee's existing bound-method representation once before constructor-export alias shims. Other bodies skip the repeated probes; native constructor exports preserve their alias dispatch. Non-constructor builtin bodies no longer require redundant per-instance entries.

Adds coverage for inherited `undefined`, non-invoked accessors, prototype changes, proxy traps, key coercion, Unicode/NUL keys, deletions, and explicit-this invocation.

This is intermediate work for #10497. The remaining function-property and invocation costs still exceed the issue’s 2× Node closure target.
