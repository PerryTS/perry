Typed-array elements reached through a class field declared as a typed array
(`a.left[i]`, `a.left[i] = v` with `left: Int32Array`) take an inline sequence
chosen by the declared element kind instead of the any-receiver indexed path.
The declaration only selects the sequence; the value's own header proves it at
every access: a heap pointer whose type byte is exactly that kind's owner brand,
an exact integer key below the live length, then a load or store at the kind's
width with no barrier and no width dispatch. Views, other kinds, detached
buffers, out-of-range or non-integral keys, non-Number stored values, lying
annotations and the BigInt kinds all leave through the one existing runtime
exit (`js_dyn_index_get` / `js_dyn_index_set_strict`). One codegen emitter
serves all eleven kinds.

A relational compare of such an element read against a Number decides the
ordered case with one `fcmp` and passes the unordered case (NaN, `undefined`,
any boxed value) to the runtime relational helper, so it is exact for every
value.

A straight-line run of statements that touch the same class-typed receiver
through fields and typed elements (`const i = a.size++; a.left[i] = l;
a.right[i] = r`) runs the receiver's inline guard once: the first site's
passing guard sets a per-activation flag, later sites test only that flag (and
take the unchanged guard call when it is clear), and every call clears it at
the call-emission choke point. Anything other than reads, local writes,
arithmetic, field updates and declared element stores ends the run.

On binary-trees with a struct-of-arrays arena this is 259G -> 109G
instructions; `check` and `make` shrink from 6.3/4.1 KB to 1.4/0.9 KB.

`PERRY_CHECK_CONTRACTS=1` while compiling makes every admitted access ask the
runtime (`js_typed_element_contract`, gc-instruments) whether it agrees, and
abort when it does not.
