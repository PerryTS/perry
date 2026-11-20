Every function object now carries a real ShapeId. A closure's header is
`{capture count, ShapeId, code pointer, own-property record}` (24 bytes on
64-bit targets); the `CLOSURE_MAGIC` payload word is gone and what a cell is
comes from its GC header's type byte. A function is born with the base
Function shape of its body kind (Function / AsyncFunction / Generator /
AsyncGenerator prototype) and moves to the shared FunctionDictionary shape
at every funnel that installs something the base shape does not describe
(a user property, a symbol key, an accessor, a delete, a recorded
`[[Prototype]]`). Function shapes live in their own ShapeId band that no
own-slot site cache accepts.

`fn.bind` / `fn.call` / `fn.apply` on a base-shaped function are decided by
the `Function.prototype` slot the shape names (identity by the slot's value,
so a patched method still wins), and a bound function is created in one
tag-checked step with its length in a capture. The method-call site reads a
callee's kind from the GC header and its code pointer at +8.

Measured against the method-call base (instructions, real release profile,
5 interleaved rounds, each arm linking its own runtime): the Zod workload
1.609G -> 1.115G (-30.7%), the tsc workload 79.57G -> 78.58G (-1.2%);
`fn.bind` 9,148 -> 3,537 instructions per call.
