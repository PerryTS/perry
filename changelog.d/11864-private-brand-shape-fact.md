Private brands and private fields are now facts of an object shape (#11791).

A class that declares private methods or accessors puts a brand in the shape
of each instance, and a private field is a private entry of the key list, so
the check behind `#x in o`, `o.#x` and `o.#m()` is a shape comparison instead
of a marker key looked up per access. The hidden marker keys and the code that
filtered them out of reflection and `JSON.stringify` are gone: `Object.keys`,
`JSON.stringify`, `Object.getOwnPropertyNames`, `Reflect.ownKeys` and `in`
read the key attributes, so a return-override stamp no longer leaks a key.

Private access through a Proxy now behaves as in node (the brand is not
forwarded to the target), and every brand-check TypeError carries node's text.

Compiled code answers a private access from that shape inline. Each `o.#x`
and `o.#m()` site keeps the last receiver shape it proved and compares it with
one load, calling into the runtime only on a miss, and a private method call
goes straight to the method body while the class has not been evaluated a
second time. Public fields of a class with private elements use the property
caches, because no finished instance is on the class's birth shape. On the
#10501 benchmark (instructions per operation, public class: 220) a private
field read and write now costs 242 (before: 494), a private method call that
updates a public field 274 (before: 1,953) and the lru-cache style class 451
(before: 1,860). A static private field is marked private where the class
creates it, and writing a read-only field of a frozen class instance names
the class in the TypeError, as node does.
