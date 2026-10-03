Fixed three gaps in classes that are created per evaluation (class expressions
in functions, and declarations whose evaluation has its own environment).
`class D extends L` now extends the L of that evaluation instead of the shared
class when the name of L is scope-renamed. A fresh class object now owns
`length`, `name` and its static methods as real own properties, so
`Object.getOwnPropertyNames`, `Object.hasOwn`, `in` and
`Object.getOwnPropertyDescriptor` see them, and `delete C.s` removes the method
from that evaluation's class only. `String(C)`, `` `${C}` ``, `"" + C` and
`C.toString()` now return the class source text.
