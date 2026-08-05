**Proxy receivers: `reverse`/`splice`/`fill`/`copyWithin` now run through the traps; receiver-less mutator thunks throw** (#6908, follow-up to #6907)

Part 1 — the remaining `Array.prototype` mutators on a Proxy receiver fired no trap and mutated nothing (the generic dispatch's receiver normalizations both reject proxy handle-band ids, so the call silently returned `undefined`; `fill`/`copyWithin` were additionally noop-backed on `Array.prototype`):

- `proxy_array_mutator` (`perry-runtime/src/array/push_pop.rs`) gains spec loops for `reverse` (§23.1.3.26) and `splice` (§23.1.3.31): `HasProperty`-gated element moves (a source hole deletes the destination), `DeletePropertyOrThrow` before the length write, carried values rooted across traps. `splice` returns a fresh real array with holes preserved.
- New `proxy_array_fill` (§23.1.3.7) backs the new real `fill` thunk and `js_array_fill_generic`'s proxy branch — a proxy receiver previously fell past both typed branches there into `js_object_coerce` self-recursion.
- `copyWithin` gets a real thunk over the already proxy-aware `js_array_copy_within_value` via a new `js_arraylike_copy_within` value entry (dense receivers keep the dense helper).
- `.call` forms pre-route proxies: `js_array_reverse_value` and `js_arraylike_splice` hand proxies to the same trap loops.
- `sort` already worked via `object_sort` over the proxy-aware `al_*` primitives; now pinned by tests.

Part 2 — a mutator prototype thunk invoked as a plain value (`const f = arr.push; f(3)`) has no receiver (`IMPLICIT_THIS` is undefined) and silently no-opped. `array_proto_mutator` now throws the node-identical `TypeError: Cannot convert undefined or null to object` for nullish receivers (spec step 1 `ToObject(this value)`), and a prior method call's receiver cannot leak into a later bare call.

Validation: 28-case probe + new gap test (`test-files/test_gap_6908_proxy_array_mutators.ts`) byte-identical to node 26.5.1; 4 new integration tests; `perry-runtime --lib` 1636/1636; 14 pre-existing proxy/array gap tests unchanged.
