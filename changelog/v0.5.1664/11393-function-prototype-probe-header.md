Plain-function construction (`new F()`) no longer re-derives `%Function.prototype%` on every call (#10602).

`is_function_prototype_object_value` guards `js_new_function_construct`, `is_constructor_value` and the
`newTarget` construct path against `new Function.prototype()`. It re-resolved the intrinsic each time:
a full `globalThis.Function` lookup plus a `prototype` dynamic-prop read, ~4k instructions, over 40% of
a plain-function construction. The intrinsic is an ordinary `GC_TYPE_OBJECT`, so a closure (every
constructor those paths see) or any other GC type is now rejected from its GC header before the lookup.
No cache or side table: the lookup still runs for the one shape that can match.

Measured with callgrind on a `function O(x){this.x=x;this.y=x+1}` / `new O(i)` loop: 2,044,055,954 →
1,265,504,687 instructions (−38.1%). A new unit test pins that `Function.prototype` stays an ordinary
object, so the pre-check cannot silently start answering `false` for it.
