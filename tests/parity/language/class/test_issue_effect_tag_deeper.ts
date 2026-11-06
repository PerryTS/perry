// Regression for the post-#915 Effect smoke gap (gap 3): factory-returned
// class + static method that reads `arguments`.
//
// Effect's `Literal(value).pipe(propertySignature, withConstructorDefault(...))`
// expands to:
//   1. `Literal(value)` — factory returning `class L extends make(ast) {}`.
//   2. `.pipe(a, b)` — static method on the returned class whose body is
//      `pipeArguments(this, arguments)` (a 2-arg curry helper that reads
//      `arguments.length`).
//
// Pre-fix, the static-method dispatch tower in
// `crates/perry-codegen/src/lower_call.rs` only recognised receivers of
// shape `Expr::ClassRef` and `Expr::LocalGet` (via class aliases).
// `Literal(value)` arrives as `Expr::Call` (or as
// `Expr::Sequence([RegisterClassParentDynamic, ClassRef])` after the
// inliner collapses the factory body inline), so dispatch fell through
// to dynamic dispatch and the synth-arguments rest slot was passed the
// raw call args one-per-slot instead of bundled into an array. Inside
// the static body, `arguments.length` therefore read `.length` on the
// first arg (a function → undefined) and the `if (arguments.length >= 2)`
// branch took the curried path with `a = undefined`, producing a
// function value where Effect expected a Schema (or PropertySignature).
// Subsequent `getDefaultTypeLiteralAST` iteration over the resulting
// `{_tag: <function>, …}` saw a function in an `_tag` slot, called
// `isPropertySignature(field)` which (via `Predicate.hasProperty`'s
// `dual(2, …)` curry shape) also mis-evaluated `arguments.length` and
// returned a curried function; the `if (isPS)` branch took that as
// truthy and read `field.ast._tag`, throwing
//   `TypeError: Cannot read properties of undefined (reading '_tag')`.
//
// This fixture covers the dispatch-side cases standalone (Effect itself
// isn't installed in test-files):
//   - Direct factory call `Literal(value).pipe(...)` — Expr::Call form.
//   - Two-deep factory chain `Outer(value).pipe(...)` — exercises the
//     `func_returns_class` fixed-point pass.
//   - Local-binding alias `const C = make(); C.pipe(...)` — the
//     pre-existing #912 (gap 2) shape, validated to still work.

// ---------------------------------------------------------------------
// Pattern A: direct factory + static-method dispatch
// ---------------------------------------------------------------------
function makeSchemaClass() {
  return class SchemaClass {
    static pipe() {
      // JS spec: `arguments.length` counts every passed arg.
      const args = arguments;
      let result: any = "BASE";
      for (let i = 0; i < args.length; i++) {
        result = (args as any)[i](result);
      }
      return result;
    }
    static getArity() {
      return arguments.length;
    }
  };
}

function Literal(_value: any) {
  // Returns a fresh class each call — exactly matches Effect's
  // `makeLiteralClass`.
  return class L extends makeSchemaClass() {};
}

const f1 = (x: any) => "f1(" + x + ")";
const f2 = (x: any) => "f2(" + x + ")";

const result_a = Literal("foo").pipe(f1, f2);
console.log("A1:", result_a);
// Expected: f2(f1(BASE))

const arity_a = Literal("bar").getArity(1, 2, 3);
console.log("A2:", arity_a);
// Expected: 3

// ---------------------------------------------------------------------
// Pattern B: transitive factory chain (mirrors Effect's
// `Literal → makeLiteralClass → returns class`)
// ---------------------------------------------------------------------
function makeOuterClass() {
  return class Outer {
    static fold() {
      return arguments.length;
    }
  };
}

function indirect() {
  return makeOuterClass();
}

function Outer(_v: any) {
  return indirect();
}

console.log("B1:", Outer("x").fold("a", "b", "c", "d"));
// Expected: 4

// ---------------------------------------------------------------------
// Pattern C: pre-existing #912 (gap 2) local-binding alias — must still work
// ---------------------------------------------------------------------
const C = makeSchemaClass();
console.log("C1:", C.pipe(f1, f2, f1));
// Expected: f1(f2(f1(BASE)))
console.log("C2:", C.getArity("a", "b"));
// Expected: 2

// ---------------------------------------------------------------------
// Next blocker for the Effect `typeof Effect.succeed === "function"` smoke
// ---------------------------------------------------------------------
// After this fix lands, `import { Effect } from "effect"; typeof
// Effect.succeed` STILL throws `TypeError: Cannot read properties of
// undefined (reading '_tag')` during Schema.ts module init. The fault
// has moved from the static-method dispatch site fixed here to a
// cross-module ExternFuncRef call chain: `Predicate.hasProperty` is
// exported as `const hasProperty = dual(2, (self, property) =>
// isObject(self) && property in self)`. `Schema.ts` calls
// `Predicate.hasProperty(field, PropertySignatureTypeId)` from inside
// `isPropertySignature(field) = hasProperty(field, PSTypeId)`, expecting a
// boolean. Empirically the call returns the dual's inner curried
// function — i.e. `arguments.length` inside the dual-returned closure
// reads as `1` (or undefined) instead of `2`. The closure is created at
// `Predicate.ts` module init by `dual(2, body)` and assigned to a const,
// so the cross-module dispatch goes through whatever stub Perry emits
// for const-exported function values — that stub is not bundling the
// args into a synthetic-`arguments` array the way `lower_call.rs`'s
// FuncRef + static-method paths now do post-#915 (gap 1) and post-this
// gap (gap 3). Likely fix area:
//   - `crates/perry-codegen/src/lower_call.rs`: ExternFuncRef call site
//     (~line 1260) needs to consult the imported module's
//     `func_synthetic_arguments` set when the imported binding's source
//     is a `dual(...)` (or any `arguments`-reading closure) and bundle
//     the args into the rest array.
//   - Or: detect at the export side that the const binding stores a
//     closure with synth-args and have `compile_module` emit a thin
//     LLVM wrapper that performs the bundling before forwarding to
//     the closure body. This avoids per-importer machinery.
// The same gap surfaces ANY time a cross-module call lands on a
// dual-returned closure, so a follow-up issue tracking this is
// warranted (file under the #321 Effect tracker).
