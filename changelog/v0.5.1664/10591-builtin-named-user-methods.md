Fix a call whose method name matched a `Date`/`Number`/`Array` builtin
(`getTime`, `toFixed`, `toISOString`, `toSorted`, `endsWith`, ...) being
lowered straight to that builtin regardless of the receiver. A class,
function-constructor prototype, or object literal defining a same-named
method — dayjs's `toISOString`/`toJSON`, decimal.js/bignumber.js's
`toFixed`, a plain `Clock.getTime()` — had its own method silently skipped
in favor of the builtin, producing `NaN`, `"[object Object]"`, `Invalid
Date`, or an uncaught `RangeError`. A zero-arg call of a user method sharing
a name with a required-arg String builtin (`endsWith`/`includes`/
`startsWith`) didn't even compile (#10476).

Add `builtin_kind_guard.rs`: a receiver the compiler has proven to be a
Date/number/array keeps the direct builtin call; any other receiver is
evaluated once, rooted, and branches at runtime on its actual kind to
either the builtin or the universal method dispatcher, which still reaches
the builtin via the prototype chain for a real Date/number/array.

Known cost: a receiver whose kind is not statically provable now pays a
real runtime dispatch check to call a builtin-named method. On a synthetic
probe this puts two `any`-typed shapes (`dayjs`-like `toISOString`/`toJSON`,
a `Money`-like `toFixed`) at roughly 3-4x Node's wall time — well outside
the usual 20%-of-Node floor. The prior fast numbers for those two shapes
were never valid: the old code crashed on one and silently computed the
wrong answer on the other, so the comparison this fix is measured against
is fix-vs-Node, not fix-vs-old-Perry.
