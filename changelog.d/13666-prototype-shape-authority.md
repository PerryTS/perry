An object's and a function's [[Prototype]] now have one record: the shape.

A null prototype was recorded twice, in the shape's identity and in a header
bit (`OBJ_FLAG_NULL_PROTO`) set at birth, and readers disagreed. Fixed: a
thenable on a relinked `Object.create(null)` object was not followed;
`util.inspect` called an object relinked to `Object.prototype` "null
prototype" and missed `Object.setPrototypeOf(x, null)`; `require.cache`
inherited `Object.prototype`'s methods. The bit is gone (0x40 is free).

`Object.setPrototypeOf(fn, p)` moved a function to the shared
FunctionDictionary shape and kept the link as a key in its state record; the
function now stays on a described shape that names the link, as an ordinary
object does. Fixed along the way: `fn instanceof C` and `fn instanceof
Function` ignored a function's link; a function linked to `null` still read
`toString` and `constructor` from `Function.prototype`; an `@@hasInstance` on
a function's linked chain was ignored; a program's first inherited read of a
function (`fn.toString`) answered `undefined`. Stores onto a bound function
take the function store lane. An inherited read on a callable class instance
drops from 1.93k to 1.64k Ir, a call from 3.70k to 3.22k; startup Ir drops on
every witness (fastify inject -0.27%, commander -0.89%).
