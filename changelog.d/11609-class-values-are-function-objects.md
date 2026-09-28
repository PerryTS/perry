### Fixed

A class used as a value is now a real function object. It used to be encoded
as the int32 number equal to its internal class id, so `1 === SomeClass` could
be true, `switch (1) { case SomeClass: }` matched, `[SomeClass, 1].indexOf(1)`
found the class, `JSON.stringify({ c: SomeClass })` printed the id and
`SomeClass instanceof Object` was `false`. Each class now has one function
object per agent, which compares, hashes, prints and reflects as it does in
Node (`[class A extends B] { statics }` in `util.inspect`).
