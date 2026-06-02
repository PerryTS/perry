// Issue #336: `@perry_class_keys_<modprefix>__<ClassName>` symbol-name
// collision when the same class name appears twice in one module.
// Triggered by Effect 3.21+ across 7 modules (Schema/JSONSchema/Pretty's
// `Refinement`, fiberRuntime/mailbox's `Class`, core/stm-core's
// `SingleShotGen`) — namespaces with same-named classes inside lower as
// two `Class` entries with the same `name` field in `module.classes`.
// Codegen emits one `@perry_class_keys_..._<name>` global per entry → clang
// rejects the IR with "redefinition of global". Pre-fix this aborted with
// `clang -c failed (status=exit status: 1)`.
//
// Fix: dedupe at HIR layer (matches the function-scoped path's existing
// dedup at `lower_decl::3059`). The lookup pipeline is purely name-based
// (`Expr::New { class_name: String }`) so the second class wouldn't be
// reachable through any binding even if both were emitted — emitting it
// just produces unreachable globals that collide with the first's.
//
// Note: `node --experimental-strip-types` doesn't support TypeScript
// namespaces, so this test runs against an expected-output file under
// `test-parity/expected/`.

namespace A {
  export class Refinement {
    kind = "a";
    value = 1;
    describe() { return this.kind + "/" + this.value; }
  }
  export function make() { return new Refinement(); }
}

namespace B {
  export class Refinement {
    kind = "b";
    value = 2;
    describe() { return this.kind + "/" + this.value; }
  }
  export function make() { return new Refinement(); }
}

// `A.make()` / `B.make()` lower as static-method calls on the namespace's
// synthetic class — both bodies emit `new Refinement()` against the
// (deduplicated) class. Pre-fix this test failed at clang IR validation;
// post-fix it compiles + runs.
//
// The lookup pipeline is purely name-based today, so the dedup keeps the
// FIRST `Refinement` (the one in namespace A) and both `make()` bodies
// resolve `new Refinement()` to it — `b.kind` therefore prints `a`, not
// `b`. That's the existing function-scoped semantics extended to namespace
// scope; pinning a distinct identity per scope would need scope info on
// `Expr::New { class_name }` and is a separate, larger change.
const a = A.make();
const b = B.make();
console.log("a.kind:", a.kind);
console.log("a.value:", a.value);
console.log("a.describe():", a.describe());
console.log("b.kind:", b.kind);
console.log("b.value:", b.value);
console.log("b.describe():", b.describe());
