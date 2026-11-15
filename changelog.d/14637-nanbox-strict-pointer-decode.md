`js_nanbox_get_pointer`, the runtime's shared decoder of a pointer VALUE (757
calls in perry-runtime, 79 in perry-stdlib, 352 in the UI crates), decides by
the tag alone: POINTER, STRING, BIGINT, JS_HANDLE and HANDLE tagged words give
their 48-bit payload, every other word gives 0. Its legacy "raw" arm, which
returned a word with clear top 16 bits as an address, is gone: such a word in a
value is a Number (a denormal double). `new Set(e)`, `new Map(e)`,
`new WeakMap(e)`, `new WeakSet(e)`, `Map.groupBy(e, f)`, `Object.groupBy(e, f)`,
`Object.groupBy([1], e)` and `String.raw(e)` no longer read an address-shaped
Number's bits as a pointer (SIGSEGV; node throws a TypeError), and a Number
whose bits equal a live object's address is no longer decoded as that object.
No caller needed the raw arm: a trace build with a positive control saw only
Numbers reach it over the parity corpus, chess, the three.js skin bench and
tssrc. No switch: the previous decode is the memory-unsafe one.
