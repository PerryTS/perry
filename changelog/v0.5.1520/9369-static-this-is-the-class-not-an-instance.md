**A class with a computed-key member no longer loses `this` inside its static
methods** — `this.prototype`, `this.<staticField>` and static-`this` writes
answer what node answers, which is what brought `cc --help` back (#9369,
unblocking #9341).

The reduced case was two answers to one question inside one function:

```js
const K = "dyn" + "Key";
class E {
  [K]() { return 1; }
  static probe() { return typeof this.prototype; }   // perry: "undefined"
}
typeof E.prototype                                   // perry: "object"
```

`class_has_computed_runtime_members` is a statement about a class's
*instances*: their key set is not described by the packed shape, so an
instance read has to go by name. Codegen applied it to every receiver whose
proven class had computed members, and `receiver_class_name` answers with the
owning class for `Expr::This` in a static body exactly as it does in an
instance body — both read `class_stack`. But a static body's `this` is the
class CONSTRUCTOR, an INT32-tagged class ref, and the by-name helper strips
the receiver NaN-box to a raw `ObjectHeader*`. The class ref's tag was masked
away, so the runtime received the bare class id as a pointer — below the
handle band, therefore not an object, therefore `undefined`. `this.name` was
the one survivor, because `js_object_get_field_by_name_f64` already reads a
small-integer receiver back as a class id for that single key.

`FnCtx::in_static_member` now records the distinction the receiver-class
answer cannot carry, and the computed-member routes (read and store) ask
before treating a proven class name as a claim about the receiver's layout.
Static bodies fall through to the general dispatch tower, which classifies
the receiver tag and already has a class-ref arm — so a static method of a
computed-member class now lowers exactly like the same method on a class
without one.

#9315 is what made this reach a real workload: it stopped giving
`[Symbol.iterator]`, `[Symbol.asyncIterator]`, `[Symbol.toPrimitive]` and
`[util.inspect.custom]` special lowering, so the common well-known-symbol
members became generic computed members. axios's `AxiosHeaders` has a
non-generator `[Symbol.iterator]()` and a `static accessor()` whose first act
is `let z = this.prototype`, and `Object.defineProperty(undefined, …)` threw
on every `cc --help`. The gap fixture
`test_gap_9369_static_this_computed_member.ts` pins all five member kinds
that take the generic path, plus static-before/after-computed, an
instance-method control, and the named-binding read that used to disagree.
