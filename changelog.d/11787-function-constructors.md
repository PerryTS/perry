Plain function constructors are cheap (#10507). A function body the compiler
emitted now carries `FN_COMPILED_BODY` in its info, so `new F()` and
`x instanceof F` decide once per body that `F` is an ordinary function and skip
the built-in, bound, proxy and native-module probes. A construction replays the
birth record kept on `F.prototype` (class id and birth ShapeId) instead of
three hash lookups and two shape interns; `F.prototype` is read through the
function's ShapeId. `x instanceof F` is one shape compare when `x`'s ShapeId
names `F.prototype`, and otherwise OrdinaryHasInstance's prototype walk, so an
object created before `F.prototype` was reassigned is no longer reported as an
instance. `instanceof` on a bound function now answers for its target. A
method call whose name a class also declares sends receivers of no such class
to the method site rather than the by-name dispatcher, so methods on a
function's prototype are called directly. Repro (instructions per op):
`new F()` 5,048 -> ~1,150, `x instanceof F` 2,809 -> ~430, decimal.js-shaped
`x.plus(i)` 15,848 -> ~4,300.
