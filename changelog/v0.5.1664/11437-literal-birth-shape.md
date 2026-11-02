perf(codegen): object literals are now born with the ShapeId module init minted for them, so `this.a` in a literal method and `lit.a` on a returned literal take the inline shape check instead of a by-name lookup (#11420). Module init minted each `__AnonShape_*` class's ShapeId before registering the class as an anon shape. The mint records the class's birth prototype, and before that registration it answered "class prototype" rather than the ordinary object prototype, so every allocation failed `try_birth_stamp_preinstalled_shape`'s prototype check and got a second, freshly minted id. Every guarded class-field read of a literal compared against the first id, always missed, and fell to `js_class_field_get_ic`'s by-name path, about 900 instructions per read. The anon-shape registrations and the generic-origin edges (the other input to a class's birth prototype) are now emitted before the keys/ShapeId loop.

Instructions per iteration (`perf stat`, slope between 1M and 5M, x86-64 release):

| loop body | before | after |
|---|---:|---:|
| `s += o.m()`, `m() { return this.a }` on a literal | 4,147 | 3,240 |
| the same method, closure body only (callgrind) | 980 | 81 |
| `claude-pkg-bench` `control/bare_loop` (`i < it.n` on a returned `{ n, warm }`) | 961 | 81 |

The remaining cost of `o.m()` on a literal (~3,100) is the generic `js_native_call_method` dispatch of the call itself, which this change does not touch.
