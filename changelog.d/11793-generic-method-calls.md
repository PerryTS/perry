**Class-method calls stay direct when the receiver is not on its class's birth
shape, and one `setPrototypeOf` elsewhere no longer retires every direct
method call in the process** (#10503, #10504, #10501, #10508).

A compiled `recv.m()` whose receiver class is known compared the receiver's
`(class_id, ShapeId)` word with the class's birth shape only. A receiver that
left it — a field added on one constructor path, a private brand, the method
surface `EventEmitter` installs, overflow storage on a wide object — missed on
every call and paid the whole dispatch tower, which re-proved the same facts
by name (an own-key string scan per call). Each such site now keeps one
learned receiver word: the miss edge (`js_native_call_method_by_id_learn`)
stores the receiver's word when its shape proves the class is the declared
class and no own key shadows the method, and a later receiver with that word
calls the body directly after one more compare. The `this.method = X`
own-override probe (#620) learns the same way
(`js_object_get_own_field_or_undef_learn`). A word is a fact of one ShapeId,
which every own key, descriptor and prototype change replaces, and it holds no
address.

`Object.setPrototypeOf` on an object that sits on no declared class chain (an
object literal, a plain function's `.prototype` as in `util.inherits`, a
`__proto__` assignment) no longer sets the all-names prototype latch; it
restarts the runtime dispatch caches only. Class instances and declared class
prototypes keep the latch.

Instructions per op (qb6, `PERRY_NO_AUTO_OPTIMIZE=1`): #10503 `if_many`
32.65M → 7.71M per tokenizer run; #10504 `setproto_chain` / `proto_assign`
27.57M → 7.47M; #10501 `private` 7,540 → 1,844, `private_field_rw` 2,011 →
485, `private_method_call` 3,478 → 1,953; #10508 `new_ee` 557,688 → 151,225,
`method_ee` 994 → 190, `emit_ee` 54,887 → 51,146. Real code is flat to slightly
better (tsc instructions −0.05%, Zod −1.4%).
