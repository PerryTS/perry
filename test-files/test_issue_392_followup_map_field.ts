// Followup to #392 (https://github.com/PerryTS/perry/issues/392#issuecomment-4357279589):
// v0.5.464 closed the structural-object-parameter shape, but the same bug
// still hit when the consumer module type-only-imports the class
// (`import type { Changeset } from "./changeset.ts"`) and the cross-module
// method calls `this.<MapOrSetField>.set/.has/.delete/.add(...)` on a field
// initialised at field-decl time.
//
// Root cause: type-only imports are stripped at HIR lowering, so the
// consumer's `hir.imports` doesn't mention the source module — and
// `compile.rs::is_unresolved_name` then treats the `Named("Changeset")`
// parameter type as resolved (it's in `all_program_type_names`), which
// short-circuits the `references_interface` full-visibility fallback that
// would have populated `imported_classes` with every program-wide class.
// Result: the consumer module's `ctx.classes` doesn't contain "Changeset",
// the static idispatch tower in `lower_call.rs` finds zero implementors of
// `set` (no Changeset stub in `ctx.methods`), the static fallback path
// also misses (no `(Changeset, set)` entry), and the `js_native_call_method`
// path was previously skipped because `class_name_opt.is_some()` —
// falling through to `js_closure_call<N>(obj.set)` which silently no-ops
// on the Map mutation.
//
// Fix in `crates/perry-codegen/src/lower_call.rs`: extend `skip_native`
// to NOT skip when the receiver's static class name is unknown to the
// codegen (i.e. not in `ctx.classes`). The runtime's
// `js_native_call_method` then dispatches via `CLASS_VTABLE_REGISTRY`,
// which v0.5.464's `js_register_class_method(...)` calls in
// `emit_string_pool` populate at module init time.
import { Changeset } from "./fixtures/issue_392_followup_pkg/changeset.ts";
import { processCommands, type Command } from "./fixtures/issue_392_followup_pkg/process.ts";

const changeset = new Changeset();

const commands: Command[] = [
  { type: "set", componentType: 1, component: { x: 1 } },
  { type: "set", componentType: 2, component: { x: 2 } },
  { type: "delete", componentType: 2 },
];

processCommands(commands, changeset);

console.log("adds size", changeset.adds.size);
console.log("has 1", changeset.adds.has(1));
console.log("has 2", changeset.adds.has(2));
console.log("removes size", changeset.removes.size);
console.log("removes has 2", changeset.removes.has(2));
