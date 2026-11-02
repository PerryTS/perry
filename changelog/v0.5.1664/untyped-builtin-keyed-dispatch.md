Builtin methods called on a receiver the compiler cannot type now take the
builtin's proving entry instead of the by-name dispatch tower. `keep.indexOf(x)`
(and `includes`, `lastIndexOf`, `at`, `concat`) on an untyped array reached the
non-string arm of the String tag guard, which called the full tower: about 4,700
instructions per call. For a key another builtin family also holds, that
arm and the array-method catch-all now take the ordinary method site: its probe
calls a user method (`vec.at(i)` on a class) directly, and a builtin receiver
takes `js_bm_<m><n>`, whose O(1) header and `Pristine` proof runs the builtin
body and sends any other receiver to the same dispatcher as before. A
String-only key keeps the dispatcher.

A computed-key method call `o[k](args)` whose arguments cannot observe the read
probes the key's atom on the receiver and its prototypes and calls a plain
function value with `this = o` directly; anything else takes
`js_native_call_method_value` as before (effect's `current[evaluate](fiber)`).

A module top-level `const` built by concatenating string literals and earlier
string consts (`const args = \`${TypeId}/args\``) binds the folded literal, so
the key is a program atom: keyed reads, writes and calls stop re-hashing it.
No runtime change, cache or side table. Gap test
`test_gap_untyped_builtin_and_keyed_calls.ts` matches node; IR and transform
tests cover each lowering.
