Whole-program facts: `new S<number>()` of a generic class that has heritage
and no constructor of its own can construct the class as written (`S`, not
its specialization), as the compiled program does when the heritage is live
(a native parent such as `Array`, `Map` or `Promise`, or a parent whose
constructor link was replaced). The facts now let that `new` construct both,
so the written class's default constructor and field initializers are no
longer claimed never to run; the run-time claim check mode reported them on
gap programs. A generic class gives the same facts as the same class
constructed without type arguments.
