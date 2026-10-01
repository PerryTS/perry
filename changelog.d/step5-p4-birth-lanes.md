Charter step 5 (P4, option (a)): a class `number` field is born on an `F64`
representation lane whenever its first write precedes every observation of the
instance under construction: a Number literal initializer, a constructor-body
store of a Number (parameter, local or arithmetic over them), in the order the
language runs construction (root first, initializers, then the body after
`super`). A `this` read, a call on `this`, an arrow capturing `this`, `super`
member access or control flow before the write keeps the field `Any`. The
field-initializer phase skips the `undefined` define of those fields. A
proven-receiver call of a typed-receiver clone now runs the clone only while
the receiver still carries its class shape, since an alias may generalize a
field.
