// Issue #6356 — a class whose parent is wired at runtime (via
// `RegisterClassParentDynamic`, i.e. any mixin/factory class that declines the
// HIR mixin fast path) must inherit the base's instance FIELD initializers and,
// through a TWO-LEVEL dynamic chain, the grandparent's methods + `instanceof`.
//
// These shapes route through `specialize_captured_class_factories`
// (`perry-transform`), which monomorphizes the returned class per call site.
// Two defects made the two-level chain fail:
//
//   1. The specialized clone reused the template class's `id` (`class.clone()`
//      copies it). Codegen keys the runtime class registry by that id, so a
//      clone collided with its template AND with sibling clones of the same
//      template — the distinct classes shared one registry slot (last-writer-
//      wins on the constructor / parent edge).
//   2. When the parent arg specialized to a runtime value (`Serializable(Mid)`
//      where `Mid` is a local binding, not a static `ClassRef`), the factory
//      Call — including the `RegisterClassParentDynamic` its Sequence carried —
//      was replaced by a bare `ClassRef`, dropping the parent edge entirely.
//
// Net symptom before the fix: `new Serializable(Greetable(Base))()` / the
// `Top = Serializable(Mid)` chain lost `.value`, `greet()`, and
// `instanceof Mid`. See the matrix in #6355 (which deliberately left these
// cells out because they matched neither node nor a clean gap).

class Base {
  value = "base value";
  hello() {
    return "base hello";
  }
}

// A canonical single-statement mixin (takes the HIR fast path when bound
// directly, e.g. `const Mid = Greetable(Base)`).
function Greetable(B: any) {
  return class extends B {
    greet() {
      return "hi";
    }
  };
}

function Serializable(B: any) {
  return class extends B {
    ser() {
      return "ser";
    }
  };
}

// A factory whose body is NOT the fast-path shape (`const K = …; return K`),
// so it declines the fast path and is factory-specialized instead.
function ViaConst(B: any) {
  const K = class extends B {
    greet() {
      return "const";
    }
  };
  return K;
}

// --- (1) return-via-const: inherited field through a dynamic parent ---------
const C = ViaConst(Base);
console.log("viaconst method:", new C().hello());
console.log("viaconst field:", (new C() as any).value);
console.log("viaconst instanceof base:", new C() instanceof Base);

// --- (2) two mixins composed in one expression (arg is a Call) -------------
const Comp = Serializable(Greetable(Base) as any);
console.log("composed field:", (new Comp() as any).value);
console.log("composed outer:", new Comp().ser());
console.log("composed inner:", (new Comp() as any).greet());
console.log("composed instanceof base:", new Comp() instanceof Base);

// --- (3) mixin over a mixin RESULT (the core two-level dynamic chain) -------
const Mid = Greetable(Base);
const Top = Serializable(Mid as any);
console.log("mid field:", (new Mid() as any).value);
console.log("mid greet:", (new Mid() as any).greet());
console.log("mid instanceof base:", new Mid() instanceof Base);
console.log("chained field:", (new Top() as any).value);
console.log("chained greet:", (new Top() as any).greet());
console.log("chained ser:", new Top().ser());
console.log("chained inherited method:", (new Top() as any).hello());
console.log("chained instanceof base:", new Top() instanceof Base);
console.log("chained instanceof mid:", new Top() instanceof Mid);

// --- (4) sibling specializations of one template must not collide ----------
// `Comp` and `Top` are both clones of Serializable's returned class; with the
// id-collision bug they shared a class id and their parent edges clobbered each
// other. Re-read both after constructing the other to confirm independence.
const compAgain = new Comp() as any;
const topAgain = new Top() as any;
console.log("sibling comp instanceof base:", compAgain instanceof Base);
console.log("sibling top instanceof mid:", topAgain instanceof Mid);
console.log("sibling comp value:", compAgain.value);
console.log("sibling top value:", topAgain.value);

// --- (5) class DECLARATION extending a factory call with a runtime-value arg
// `class Child extends Serializable(V)` rewrites the factory Call into a
// `Sequence([RegisterClassParentDynamic, ClassRef(clone)])`. The decl-time
// `RegisterClassParentDynamic { class_name: "Child", parent_expr: <that
// Sequence> }` must still hoist the clone as `Child`'s static parent (the hoist
// resolves through the Sequence's trailing `ClassRef`), so `Child` inherits the
// base's field/method through the specialized clone.
const V: any = Base;
class Child extends Serializable(V) {
  child() {
    return "child";
  }
}
console.log("decl field:", (new Child() as any).value);
console.log("decl inherited method:", (new Child() as any).hello());
console.log("decl own method:", new Child().child());
console.log("decl instanceof base:", new Child() instanceof Base);
