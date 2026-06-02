// Regression for #392: when a class instance is passed as an argument
// to a function defined in another module (with a structural-object
// parameter type), method calls on the instance inside that function
// silently failed. The receiver's runtime class_id and the runtime's
// VTABLE_REGISTRY were correctly set up, but the codegen never emitted
// `js_register_class_method(...)` calls for any class — the FFI was
// declared but no caller invoked it. So `js_native_call_method` always
// fell through past the vtable lookup.
//
// Same-module calls worked because the codegen's static idispatch
// tower in `lower_call.rs` enumerates `ctx.classes` to find
// implementors of the called method and emits a class_id check that
// dispatches to `perry_method_<class>__<method>` directly, bypassing
// the vtable entirely.
//
// Fix: in `crates/perry-codegen/src/codegen.rs::emit_string_pool`, after
// the existing `js_register_class_parent` block, emit a
// `js_register_class_method` call per (class, method) pair for every
// class DEFINED in the current module (skipping imported class stubs
// via the `method.body.is_empty()` check). The runtime side
// (`perry-runtime/src/object.rs::js_native_call_method`) already
// looked up the vtable; the codegen side just wasn't populating it.
import { Changeset } from "./fixtures/issue_392_pkg/shared.ts";
import { processCommands } from "./fixtures/issue_392_pkg/commands.ts";

const changeset = new Changeset();
processCommands(
  [
    { type: "set", componentType: 1, component: "x" },
    { type: "set", componentType: 2, component: "y" },
  ],
  changeset,
);

console.log("size:", changeset.adds.size);
console.log("has 1:", changeset.adds.has(1));
console.log("has 2:", changeset.adds.has(2));
console.log("has 3:", changeset.adds.has(3));
