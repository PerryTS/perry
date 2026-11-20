Reading a property before its first write no longer makes every later read
answer `undefined`.

```ts
const c: any = {};
const b = c["k"];   // read before the key exists
c["k"] = 7;
c["k"]              // was undefined, now 7
```

Scalar replacement gives each DECLARED field of a non-escaping `new` its own
alloca (`stored_fields = all_fields ∩ used_fields`). An object literal lowers to
a synthetic `__AnonShape_` class whose `all_fields` is exactly the literal's
keys, so a write to a key the literal does not have had nowhere to go: the store
was dropped and the reads saw the alloca's initial `undefined`.

`escape_check.rs` escaped such a receiver for a class setter and for a
self-referential value, but neither the `PropertySet` nor the `PutValueSet` arm
checked that the written property is a declared field. Both now do, via
`class_chain_has_field`, which fails toward "has it" for an unmodeled base and
for a class carrying computed members — so it can only escape more
conservatively than the declared list justifies, never less.

Not specific to object literals: a user class took the same path, so
`class C { x = 1 }` followed by `(new C() as any)["k"] = 7` read back wrong too.

The failure was silent rather than a crash, on an ordinary shape:
`if (!cache["v"]) cache["v"] = compute(); return cache["v"];` returned
`undefined`. Covered by `test-files/test_gap_scalar_replace_new_field.ts`.
