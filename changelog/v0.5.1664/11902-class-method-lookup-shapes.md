A by-name method call on a class instance (an untyped receiver, a computed
key `obj[k]()`, or a compiled class-method arm's miss edge) now finds the
method on the instance's prototype chain by shapes: each prototype's key list
names the holder and the slot, and the slot's ConstFn lane names the body. The
runtime's per-(class id, name) dispatch caches `VTABLE_IC` and
`OBJ_DISPATCH_IC` are deleted. A computed-key call site and a compiled arm's
miss edge keep a chain memo of the walk that answered them (the receiver's
word, every prototype's word, the holder's slot), compared word by word on
every use: `obj[key]()` drops from about 1,275 to 753-800 instructions per
call at holder depth 1-6, and the #10507 `decimal_class` row from 1,447 to
785.
