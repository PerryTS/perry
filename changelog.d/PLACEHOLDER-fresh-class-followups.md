Fixed three conversion and reflection gaps in classes created per evaluation
(class expressions in functions, and declarations whose evaluation has its own
environment), and moved their shape memo out of a table.

`String(C)` on such a class now returns the class source after `delete
C.toString` removed a static `toString` the class declared, instead of
`[object Function]`. An own `toString` set to a non-callable value such as
`undefined` makes `String(C)` throw a `TypeError` again, as in node, instead of
returning the class source. `Object.getOwnPropertyNames(C)` lists integer keys
such as a static method named `0` before `prototype`, as in node.

What a class remembers to build later evaluations directly in its shapes now
lives in the class's own record instead of a table keyed by class.
