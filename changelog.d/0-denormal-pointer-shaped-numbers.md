A JS Number whose IEEE bits look like a raw heap pointer (every denormal
double, e.g. `Number.MIN_VALUE * 268435456`, and `+0`) is a Number in every
value-typed runtime entry touched here, decided by the tag alone: in value
context an object carries POINTER_TAG, so a word with clear top 16 bits is
never an object reference. Reading or writing a property on such a value
(`e.x`, `e["x"]`, `e.x = 5`, `e.x++`), a symbol-keyed read, using it as a
property key, a `Map`/`Set` key or an identity-compare operand, `instanceof`,
`Array.isArray`, `Function.prototype.apply` argument lists, spread and the
`Object.*` / `Reflect.*` entries that decode through `extract_obj_ptr` /
`object_ref_addr` no longer read the Number's bits as an address (SIGSEGV), and
a Number whose bits equal a live object's address is no longer read or written
as that object (type confusion). Codegen: a named read whose receiver is not
proven to be an object takes the value entry (`js_object_get_field_value`) for
such a Number instead of handing its bits to the by-name handle entry, and the
computed-access handle no longer carries a Number's own bits. No switch: the
previous lowering is the memory-unsafe one.
