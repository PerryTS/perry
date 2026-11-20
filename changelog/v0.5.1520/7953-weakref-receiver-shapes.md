Gave `WeakRef` and `FinalizationRegistry` an actual runtime method surface, and
made every folded weak intrinsic brand-check its receiver.

`WeakRef.prototype.deref` and `FinalizationRegistry.prototype.register` /
`.unregister` had **no runtime existence at all**. They were purely an HIR fold:
`pre_scan_weakref_locals` records bare local NAMES bound to
`let/const x = new WeakRef(…)` — walking module statements and the bodies of
function *declarations* only — and `expr_call/url_date_instance.rs` folds
`<tracked-name>.deref()` to `Expr::WeakRefDeref`. Anything the fold could not
name fell through to ordinary dynamic dispatch, where nothing resolved:
`try_weak_method_dispatch` early-returned unless the receiver carried
`CLASS_ID_WEAKMAP`/`CLASS_ID_WEAKSET`, `install_collection_proto_methods` had no
arm for either wrapper (so `WeakRef.prototype` carried no `deref` property at
all), and the by-name value read in `get_field_by_name.rs` had a WeakMap/WeakSet
arm but no wrapper arm.

Measured over twenty receiver shapes, **two worked**: a `const x = new WeakRef(…)`
at module top level, and the same inside a function *declaration*. Everything
else threw `TypeError: deref is not a function` — an array element (#7947's
report), a local copied from one, an object property, a call result, a `for…of`
binding, a function parameter, a `.map` callback, `new WeakRef(x).deref()`
inline, a `Map` value, and any binding inside an arrow function, function
expression or class method. The reflective path was equally dead:
`WeakRef.prototype.deref.call(wr)` threw "was called on a value that is not a
function", `wr.deref.bind(wr)` threw "Bind must be called on a function",
`typeof wr.deref` was `undefined`, and `wr.deref?.()` silently produced
`undefined`. `WeakMap`/`WeakSet` passed every one of those shapes both before and
after, because they have all three routes — that asymmetry is the whole bug, and
`weakref_locals.rs` already named it in a comment justifying why those sets are
exempt from the ambiguity poison pass ("have no runtime method-dispatch
fallback — they rely on the codegen fast path").

Three additions close it. `try_weak_method_dispatch` gains
`("deref", CLASS_ID_WEAKREF)` and
`("register" | "unregister", CLASS_ID_FINALIZATION_REGISTRY)` arms, so a *call*
on any receiver shape reaches the runtime helper. Brand-checking prototype
thunks are installed on both prototypes via
`populate_builtin_prototype_methods`, which fixes `.call`/`.apply`, method
extraction, the spec `.length` values, and makes
`WeakRef.prototype.deref.call({})` throw the `TypeError` the spec requires. And
`get_field_by_name.rs` resolves those same thunk values for an instance read, so
`typeof wr.deref === "function"` and `wr.deref === WeakRef.prototype.deref`.
`Object.prototype.toString` gained the two missing arms as well
(`[object Object]` → `[object WeakRef]` / `[object FinalizationRegistry]`).

The same investigation turned up the silent sibling, filed as #7948 and closed
here. The fold is name-keyed and **scope-blind**, and its helpers did not
brand-check, so one genuine `const r = new WeakRef(x)` anywhere in a module
folded *every* `r.deref()` in that module onto `js_weakref_deref` — which read
`__perry_wr_target` by name off whatever it was handed and answered `undefined`.
A user class instance, an object literal, an array with an attached `deref`, and
a **function parameter** all silently returned `undefined` instead of their own
method's result, with exit code 0. `weakmap_locals`/`weakset_locals`/
`proxy_locals` *are* subtracted by the ambiguity poison pass, but that pass only
recognises `new <OtherClass>()` and call/await initializers — it cannot see an
object literal, an array, or a parameter, so the identical hijack went through on
the far more common `get`/`set`/`has`/`add`/`delete`. Name poisoning can only
ever be a partial patch, because the pre-scan cannot enumerate every way a name
acquires a non-intrinsic value; parameters and destructuring bindings are not
even declarations it visits. So the fix went on the other side:
`js_weakref_deref`, `js_finreg_register`, `js_finreg_unregister`,
`js_weakmap_{set,get,has,delete}` and `js_weakset_add` now verify the receiver's
reserved `class_id` before trusting it and hand a foreign one to
`dispatch_foreign_weak_receiver`, which re-enters `js_native_call_method`. A
mis-fold degrades to the correct slow path instead of a wrong answer. Recursion
is impossible: `js_native_call_method` routes back into these helpers only via
`try_weak_method_dispatch`, which requires exactly the reserved `class_id` the
brand check just rejected.

`try_weak_method_dispatch` and `weak_class_id_from_receiver` moved out of
`weakref.rs` (at 1988 of the 2000-line gate) into a new
`crates/perry-runtime/src/object/weakref_proto_thunks.rs` as a pure move, then
extended there.

Deliberately left: weak-wrapper **subclassing**. `class M extends WeakMap {}` and
the WeakSet/WeakRef/FinalizationRegistry equivalents throw before *and* after this
change (`value is not a function`; `Constructor WeakRef requires 'new'`),
verified identical against a pristine `origin/main` binary so the new brand
checks cannot be blamed for it. It is a different mechanism —
constructor/prototype reification, of which `map_set_subclass` exists only for
Map/Set — and the gap test's header pins it as an explicit non-boundary so a
green run is not misread as coverage. The HIR pre-scan is also still name-keyed
and scope-blind and still does not descend into arrow bodies; that is now a
*performance* property rather than a correctness one, since an unnamed receiver
takes the dynamic path and a mis-named one brand-checks its way back to the right
method.

`test-files/test_gap_weakref_receiver_shapes_7947.ts` pins all of it: the 14
previously-throwing receiver shapes, the reflective and value-read paths, both
`toString` tags, the brand check, `FinalizationRegistry` through three shapes,
the six WeakRef/FinReg name-collision cells and the five WeakMap/WeakSet ones,
and the WeakMap/WeakSet shapes that already worked — so a future refactor of the
shared dispatch cannot silently drop them.
