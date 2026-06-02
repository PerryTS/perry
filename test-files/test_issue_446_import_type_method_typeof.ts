// Regression test for #446 (https://github.com/PerryTS/perry/issues/446):
// `obj.method` PropertyGet on a class instance returned `undefined` for any
// method reference. `typeof obj.method === "undefined"` and `let f =
// obj.method` produced a non-callable value. The 1f0bbfc3 followup
// (v0.5.476) had restored the *call* path through CLASS_VTABLE_REGISTRY so
// `obj.method(args)` worked, but property-as-value still fell through
// `crates/perry-codegen/src/expr.rs::PropertyGet` to the generic property-bag
// lookup which only sees own-fields — class methods live in the prototype
// vtable, not the object's field bag.
//
// Compounded for `import type { Foo }` (whole-declaration type-only):
// `crates/perry-hir/src/lower.rs::lower_module_decl` early-returned on
// `import_decl.type_only` before any specifier flowed class metadata into
// `imported_classes`. The runtime CLASS_VTABLE_REGISTRY fallback (#392
// followup) saved the call path, but typeof / property-as-value had no
// fallback. @codehz/ecs's `world.set` path tripped this in the wild.
//
// Fix in three places: (a) `crates/perry-runtime/src/object.rs` — new
// `js_class_method_bind(instance, name_ptr, name_len)` allocates a 3-capture
// `BOUND_METHOD_FUNC_PTR`-sentinel closure (same machinery that backs
// `js_native_module_bind_method`); the closure dispatches through
// `js_native_call_method` → CLASS_VTABLE_REGISTRY for both local and
// cross-module classes. (b) `crates/perry-codegen/src/expr.rs::PropertyGet`
// — after the field-fast-path GEP+load, before the generic fallback, when
// `(class_name, property)` is in `ctx.methods`, emit `call double
// @js_class_method_bind(recv, ptrtoint(@.str.<N>.bytes), <len>)`. The bytes
// pointer is per-module rodata, stable for the program's lifetime — no
// per-call allocation. (c) `crates/perry-hir/src/lower.rs::lower_module_decl`
// — drop the unconditional early-return on `import_decl.type_only`; only
// short-circuit for native modules (no class info there). For TypeScript
// modules, fall through and propagate `whole_decl_type_only ||
// named.is_type_only` into the existing per-specifier `is_native` skip
// (mirrors the v0.5.405 per-specifier `import { type Foo }` fix).
//
// Test exercises all three named-import shapes (`import type { Inner }`,
// `import { type Inner }`, `import { Inner }`) plus a main-module
// value-imported usage. Pre-fix:
//   typeof inner.setAdd (type-only)    : undefined
//   typeof inner.setAdd (inline-type)  : undefined
//   typeof inner.setAdd (value-import) : undefined
//   typeof inner.setAdd (main)         : undefined
// Post-fix all four report "function" matching `node --experimental-strip-types`.
import { Inner } from "./fixtures/issue_446_pkg/inner.ts";
import { consumeTypeOnly } from "./fixtures/issue_446_pkg/consumer_type_only.ts";
import { consumeInlineType } from "./fixtures/issue_446_pkg/consumer_inline_type.ts";
import { consumeValue } from "./fixtures/issue_446_pkg/consumer_value.ts";

const inner = new Inner();

console.log("typeof inner.setAdd (type-only)    :", consumeTypeOnly(inner));
console.log("typeof inner.setAdd (inline-type)  :", consumeInlineType(inner));
console.log("typeof inner.setAdd (value-import) :", consumeValue(inner));
console.log("typeof inner.setAdd (main)         :", typeof (inner as any).setAdd);

console.log("adds.size                          :", inner.adds.size);
console.log("adds.get(1)                        :", inner.adds.get(1));
console.log("adds.get(2)                        :", inner.adds.get(2));
console.log("adds.get(3)                        :", inner.adds.get(3));
