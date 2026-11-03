A name-patched method call now honours a replacement on a FUNCTION receiver
again. `Function.prototype.toString = f; fn.toString()` ran `f` before #11915;
after it the call threw `TypeError: value is not a function`, because the
redirect's shared generic path is `Call(Get(recv, name), recv, args)` and that
`Get` (`method_site::spec_get`) runs `js_object_get_field_by_name_f64`, whose
closure arms report `toString`/`valueOf`/`isPrototypeOf`/`call`/`apply`/`bind`
absent — `%Function.prototype%`'s method surface belongs to dedicated
dispatch — so the redirect read the absence as "not callable".

`spec_get` now resolves the name off the receiver's real `[[Prototype]]` when
the generic get reports it absent, the same `Get`
`closure::reify_function_method_value` already performs for
`call`/`apply`/`bind`: a replaced slot is the value the call runs, a deleted
slot stays absent (its `TypeError` is preserved), an accessor-installed
replacement resolves through its getter, and a detached function
(`Object.setPrototypeOf(fn, null)`) still finds nothing. Only closure
receivers take the fallback, and a value that is still the runtime's builtin
keeps the existing `is_builtin_method` answer, so every other builtin family's
redirect and the by-name native arm are unchanged.

Fixes the last failing `globals` fixture,
`test-parity/node-suite/globals/function-tostring-override.ts` — the module now
runs 124/124 against the pinned oracle (node v26.5.1).
