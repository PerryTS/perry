fix(hir): a per-evaluation class declaration's own members see its evaluation, not the shared template (#11157)

bson's `ObjectId` (mongodb 7.5.0 installs bson 7.3.3) gave the same id on every `new ObjectId()` when compiled through `perry.compilePackages`: the counter was always 1 and the 5 `PROCESS_UNIQUE` bytes were all 0. mongodb's `insertMany` then failed with `E11000 duplicate key`.

Root cause: `class ObjectId extends BSONValue` sits inside `bson.cjs`, and a CommonJS module body is a function body. Its base `BSONValue` reads module-level consts, so `BSONValue` is a capturing class value and `ObjectId`'s heritage is a runtime value (`extends_expr`). That sends the declaration down the per-evaluation `ClassExprFresh` path, and its statics (`index`, `PROCESS_UNIQUE`) become own properties of the evaluated class object. The class's own members still resolved the name `ObjectId`, and `this` in a static-field arrow, to the shared template:

- `ObjectId.index = (ObjectId.index + 1) % 0x1000000` in `static getInc()` lowered to a template-keyed `StaticFieldSet`, with `StaticFieldGet` for the read. It read and wrote the template global, never the object whose `index` the static block had set.
- `static resetState = () => { this.index = …; this.PROCESS_UNIQUE = null }` had `this` replaced by `ClassRef(template)`. `this === ObjectId` was false in the static block, and the reset wrote to the template.
- `ObjectId.PROCESS_UNIQUE ??= ByteUtils.randomBytes(5)` in the constructor stored the random bytes where the next read never looked, so the packed id read `undefined` bytes, which came out as 0.

Named class *expressions* already had a compiler-private self-binding local (`class_expr_self_bindings`) that codegen fills with the evaluated class object before any static initializer runs. Class *declarations* did not. The fix:

- `lower_class_decl` registers the same self-binding for a function-body declaration that takes the fresh path because of runtime heritage or private elements. The declaration arm asks for it through `class_decl_self_binding_wanted`. Members then resolve the class name to this evaluation, and `this` in static-field initializers becomes the self-binding. `substitute_lexical_this_in_expr` now adds a local replacement to each rewritten arrow's capture list.
- The `ClassExprFresh` for the declaration carries the binding as its `evaluation_owner` when a static initializer or capture uses it. The owner is registered with the enclosing body's class-expression owners, so it is declared and rooted at body entry.
- The template-keyed `StaticFieldGet`/`StaticFieldSet`/`StaticMethodCall` fast paths no longer fire for a self-binding or for a declaration recorded as per-evaluation (`per_evaluation_class_decls`). Those accesses go through the class value.

Module-top classes and function-body classes that stay on the shared-template path are unchanged. A unit test asserts that the control keeps its `StaticFieldSet`.

Validation (Linux x64, perry-dev, Node 26.5.1):
- `test-files/test_gap_11157_class_decl_self_statics.ts` fails on main at 93a86ffb, passes with the fix, and its output is byte-identical to Node.
- bson 7.3.3 repro from the issue: main gives `distinct 1 rand-zero true counter-step 0`. With the fix it gives `distinct 3 rand-zero false counter-step 1` both with `PERRY_NO_AUTO_OPTIMIZE=1` and with auto-optimize on, which matches Node.
- `lower::tests::issue_11157_class_decl_self_statics` has 2 tests. The per-evaluation test fails with the source change reverted and passes with it.
