Fixed gaps in classes that are created per evaluation (class expressions in
functions, and declarations whose evaluation has its own environment), and made
creating them cheap.

`class D extends L` now extends the L of that evaluation instead of the shared
class when the name of L is scope-renamed. A fresh class object now owns
`length`, `name` and its static methods as real own properties, so
`Object.getOwnPropertyNames`, `Object.hasOwn`, `in` and
`Object.getOwnPropertyDescriptor` see them, and `delete C.s` removes the method
from that evaluation's class only. Each evaluation's static methods and
prototype methods are its own function objects, and they run in that
evaluation even when called detached (`const f = C.s; f()`) or with another
receiver. Deleting a method from one evaluation's prototype no longer removes
it from other evaluations or from classes evaluated later. `String(C)`,
`` `${C}` ``, `"" + C` and `C.toString()` now return the class source text.

Every evaluation after a class's first is built directly in the class's
recorded shapes: its class object and its prototype are each allocated in
their final shape and filled, without per-property definitions. Evaluating a
class with a captured variable costs about 5,600 instructions instead of about
25,000, and each static method adds about 660 instead of about 19,800.
