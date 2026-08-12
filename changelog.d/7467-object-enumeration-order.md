fix(runtime): own-property enumeration order + observable trap order for `Object` statics; array symbol-keyed properties (#5901, PR #7467)

- Symbol keys now keep property-**creation** order across a data→accessor
  `defineProperty` redefine, and for accessors installed between two data
  installs: `set_symbol_accessor_property` leaves an order-preserving
  placeholder in `SYMBOL_PROPERTIES` (value readers all consult the accessor
  table first; `clone_symbol_entries_for_obj_ptr` filters placeholders for
  the raw-entry consumers). test262:
  `getOwnPropertySymbols/order-after-define-property`.
- `Object.values` / `Object.entries` on a Proxy fire one `ownKeys` trap, then
  interleave `getOwnPropertyDescriptor` + `get` per key per
  EnumerableOwnPropertyNames, instead of batching all descriptor reads first.
  test262: `values/observable-operations`, `entries/observable-operations`.
- `Object.getOwnPropertyDescriptors` on a Proxy fires `ownKeys` once (the
  generic string/symbol two-helper enumeration fired an observable second
  trap) and reads descriptors in the trap result's verbatim key order.
  test262: `getOwnPropertyDescriptors/observable-operations`.
- Arrays support symbol-keyed properties: `arr[sym] = v` was silently
  dropped (no symbol arm in `js_array_set_index_or_string`) and `arr[sym]`
  hard-returned `undefined`; both now route through the symbol side table
  like plain-object receivers.

Validation: new sabotage-verified unit test
(`symbol_keys_keep_creation_order_across_accessor_redefine`); perry-runtime
`--lib` 1655/1655; test262 `built-ins/Object` slice 3141→3149 pass with only
removals in the failure diff; `built-ins/Array` slice swept — remaining
failures all predate the change (#5898 snapshot cross-check).
