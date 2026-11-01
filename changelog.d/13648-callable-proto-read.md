Read an inherited property of a callable class instance (a function whose
[[Prototype]] is a class prototype, as ArkType's `Callable` builds with
`Object.setPrototypeOf(fn.bind(this), C.prototype)`) as one OrdinaryGet with
the function as receiver: the prototype's layouts answer a data value or the
getter to call (a compiled class getter or a getter closure, run with the
function as `this`), and only what they decline takes the complete read. An
inherited method read drops from 16.8k to 1.9k Ir and an inherited getter from
7.8k to 2.8k; Supabase CLI `--version` startup drops 13.5% (8.09 G to 7.00 G
Ir). The walk over a function's recorded prototype chain moved out of line of
the keyed function-property read, so that read's common path got shorter.

`Object.getPrototypeOf` on an instance of a declared class (and every runtime
prototype walk, such as the store check on a function's chain) now reads the
class's prototype holder from the instance's shape, the link the layout probe
already follows, instead of the generic resolution tail.

Fixes: a data `null` on a callable's prototype chain read as `undefined`, and
a key absent from that chain could find an expando on `Function.prototype`,
which is not on it.
