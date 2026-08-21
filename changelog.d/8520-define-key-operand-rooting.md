**GC: `Object.defineProperty` / `Reflect.defineProperty` filed the property under `"[object Object]"` when the key's coercion collected (#8507).**

A key object whose `toString` allocated enough to trigger an evacuating collection ended up storing the property under `"[object Object]"` instead of its real name:

```ts
const key = { n: "label", toString() { churn(); return this.n; } };
const ta = new Int32Array([1, 2, 3]);
Object.defineProperty(ta, key, { value: payload, configurable: true });
Object.getOwnPropertyNames(ta);   // was ["0","1","2","[object Object]"], now [...,"label"]
```

**Cause.** Both entry points probe the TypedArray integer-index path first, and that probe coerces the key inside `canonical_index_for_key` — which runs the user `toString` and can therefore evacuate. Every operand below the probe was still being read from the raw parameters, so they named pre-move addresses. Instrumenting `js_string_coerce` shows the signature directly: two coercions of the **same** pointer returning different answers.

```
[coerce] ptr=0x…94850 -> "loudkey"
[coerce] ptr=0x…94850 -> "[object Object]"
```

A moved key object still parses as an object at its old address, but its own `toString` is no longer reachable there, so the second coercion silently falls back to `Object.prototype.toString`. Both entry points now root the three operands above the probe and re-read them afterwards; `reflect_define_property` additionally re-reads between `array_length_reflect_define`, `obj_value_has_own_key` and `obj_value_attrs`, each of which re-coerces the key.

The `Reflect` half was not merely a lost value: a stale key made the own-key probe answer `false`, which skipped the non-configurable guard entirely and **accepted a redefine that must fail** — a dropped spec invariant, not just a missing property.

**Why it surfaced now.** The suite's own header calls itself a behavioural guard that "starts failing the day a minor-evacuating configuration becomes reachable from compiled code"; #6946 made an explicit `gc()` run an evacuating minor first. These are real defects newly exposed, not regressions.

`gc_string_coerce_property_key_rooting_6943` goes 1/3 → 3/3. Also verified green: `gc_property_key_operand_rooting_6935` (3/3), `gc_dynamic_arith_operand_rooting_6655` (4/4), `gc_side_table_roots_evacuation` (1/1).

Closes #8507.
