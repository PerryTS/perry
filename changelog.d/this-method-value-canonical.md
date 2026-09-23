**Behaviour change: a method read off `this` is the class's method, not a
receiver snapshot.** `const f = this.m` now answers the same canonical
function as `obj.m` and `C.prototype.m` (`this.m === C.prototype.m` holds),
and the value binds no receiver — calling it bare runs with `this`
undefined, as in Node. This retires the #4548 snapshot contract, under
which every `this.m` read built and named a fresh bound closure and a
captured `this.m` kept its receiver after an own-property replacement. The
constructor self-rebind `this.m = this.m.bind(this)` that #4548 fixed keeps
working. zod: −26.8% instructions (its `ZodType` constructor reads and binds
twenty inherited methods per schema).
