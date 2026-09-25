perf(codegen): `this.field` inside an object-literal method now costs the same as in a class method (#10906). A closed-shape literal lowers each method to a dynamic-`this` closure stored in a field of its `__AnonShape_*` class, so two things differed from a class method's `%this_arg` receiver: every `this` was a separate `js_implicit_this_get_sloppy()` call (a TLS read plus nullish/primitive checks, a safepoint), and every `this.field` took the generic per-site inline cache, because nothing tied the closure's `this` to the literal's class.

- **One receiver read per call.** A non-arrow, non-async, non-generator closure with its own `this` binding (a literal method or a function expression) now reads `IMPLICIT_THIS` once in its prologue into a shadow-rooted `this` slot, after its parameters, captures and `arguments` are rooted and before the first statement. That is what OrdinaryCallBindThis does. It also means the sloppy conversion runs once, so `this === this` holds for a primitive receiver, and an arrow inside a sloppy method called bare now sees `globalThis` (matching node) instead of `undefined`.
- **Guarded class-field access.** `collectors::literal_method_home_classes` maps each such method closure to the shape class its literal allocates. That class is only a *guarded candidate* for `this` (`FnCtx::guarded_this_class`), at the same evidence level as a `Named` local type hint. It nominates the guarded class-field get/set paths, whose runtime class-id/shape check keeps `f.call(other)`, an extracted method, `Object.create(lit)`, spreads, shape transitions, frozen receivers and accessors correct. `receiver_class_name` never sees it, so no proven-receiver path does either.

Marginal instructions per loop iteration (cachegrind, x86-64-v3, prebuilt runtime; the issue's `SINK.push(this)` fixtures):

| operation | literal method, before | literal method, after | class method |
|---|---:|---:|---:|
| `read1`: `this.d = k; h += this.a` | 191 | **96** | 101 |
| `read4`: store + four reads | 520 | **264** | 252 |
| `overwrite`: `this.d = k` | 97 | **58** | 59 |
| `read1_hoisted`: `h += this.a` | 96 | **40** | 40 |

The class spelling is unchanged. Validation: `test-files/test_gap_10906_object_literal_method_this.ts` pins the receiver semantics against node, and new `codegen::literal_method_this_tests` cover one rooted entry read (sloppy and strict), the guarded shape-class read, arrows left alone, and home-class ambiguity. Three of them fail with the entry read disabled. perry-codegen lib 1711/0. A node differential over foreign/primitive/nullish receivers, shape transitions, freeze, accessors, nested arrows/functions, throwing callees and allocation pressure is byte-identical to node, including under `PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1`, apart from one pre-existing, unchanged gap: a `"use strict"` directive inside a literal method body is not honoured.
