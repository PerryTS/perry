Fixed a `new F()` instance reading F's *current* `.prototype` instead of its own
`[[Prototype]]` after `F.prototype` was reassigned (#11391):

```ts
function F() {}
F.prototype = { a: 1 };
const o = new F();
F.prototype = { a: 2, b: 3 };
o.b;   // was 3, now undefined (as in node)
```

The instance carries F's synthetic class id and a class-default link to the
prototype recorded at construction. On an own-key miss the object-getter tail
asked the class-id walk (`resolve_proto_chain_field_noting_miss` and
`lookup_prototype_method`) first. That walk reads `CLASS_PROTOTYPE_OBJECTS[F]`,
which is F's current prototype, so it answered from an object that is not on
`o`'s chain.

- `prototype_chain::class_default_prototype_superseded` detects the divergence
  from the instance's own meta record: a recorded pointer prototype on a
  synthetic class id that is no longer the class id's prototype object.
- `prototype_override::inherited_field_if_overridden` then treats the recorded
  chain as authoritative, the same way it does for a per-instance override. A
  miss returns the new `InheritedRead::Superseded`, and both tail arms (keyless
  and shaped receivers) skip the class-id prototype arms for it. A recorded
  chain that ends in an explicit `null` answers `undefined`, as for #10827.
- `js_native_call_method` routes such a receiver through the ordinary property
  read before the class dispatch tower, so `o.m()` calls the method on the
  recorded prototype rather than on F's new prototype.

When the recorded prototype is still F's prototype, nothing changes: the
class-id walk reads the right object.

Regression coverage: `test-files/test_gap_function_prototype_reassigned_instance.ts`
checks keyless and shaped receivers, bracket reads, `in`, `constructor`,
accessors and methods on the old prototype, a `F.prototype = new G()` chain,
and instances built after the reassignment.
