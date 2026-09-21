fix(runtime): calling the function an instance getter returns no longer throws "is not a function" (#10893).

`c.g(1)`, where `g` is an instance getter returning a function, threw `TypeError: g is not a function` — while `const f = c.g; f(1)` returned that very function. The value was right; only the combined member-call form missed.

`js_native_call_method`'s dispatch tower probes vtable methods, own fields and the prototype chain for a callable VALUE, but never RUNS a getter, so an accessor-exposed callable fell through every arm. The runtime's own diagnostic named it: `call-method (no method/field/proto match)`.

Fix: an accessor arm at the END of the tower — read the property through the ordinary by-name get, which runs the accessor, and invoke the result with the receiver bound as `this`. Being last, a real method of the same name still wins, and a getter yielding a non-callable still throws as before. Object-literal getters and plain static getters already worked and are pinned by the test.

`x.someGetter(...)` is ordinary JS — lazily built handlers, memoised factories and "return a bound function" accessors all use it — and the failure named the property as if it were a missing method, which sends you looking for the wrong thing.
