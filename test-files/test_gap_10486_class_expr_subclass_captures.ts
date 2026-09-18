// #10486: an inherited method of a capture-bearing class EXPRESSION saw
// `undefined` captures when called on an instance of a capture-bearing
// `class extends` subclass with no explicit constructor.
//
// `lower_class_decl`/`lower_class_from_ast` (crates/perry-hir/src/lower_decl/
// class_decl.rs) deliberately leave `extends_name` at `None` when the
// heritage identifier resolves to a lexically-scoped local (`locally_
// shadowed`) rather than a statically-registered class declaration — the
// #5437 PQueue fix, avoiding a retained name being re-resolved by the
// static parent-chain walks to an unrelated same-named class.
// `synthesize_class_captures` (lower_decl/class_captures.rs) unions a
// parent's registered captures into the child's synthesized-constructor
// params keyed off that SAME `extends_name`; when it's `None`, the union
// never ran, so the subclass constructor never received the base's
// captured locals and every inherited method read `undefined` for them.
//
// This is exactly the shape `const Base = class {...}; const Sub = class
// extends Base {...}` produces (both class EXPRESSIONS assigned to
// locals) — esbuild/tsc's typical bundled-class emit, and what broke
// typescript 5.8.2's CJS `transpileModule` (`IdentifierNameMultiMap
// extends IdentifierNameMap`, both class expressions in `typescript.js`'s
// module wrapper, the subclass with its own `add`/`remove` methods).
//
// NOTE: this fix is scoped to a subclass that has at least one member of
// its own (a method, in this file) and NO explicit constructor of its own
// — see `explicitCtor` below, the pre-existing "already works" control
// this fix must not regress. A subclass with a completely empty body
// (`const Sub = class extends Base {};`), or a base class DECLARATION
// (rather than expression) as the extends target, hits a SEPARATE, unfixed
// codegen field-layout bug — see the PR body for #10486.
//
// Each function below uses its own distinct Base/Sub identifier names
// (Base1/Sub1, Base2/Sub2, ...): re-using the same literal name across
// sibling functions hits an unrelated, pre-existing collision in the
// class-capture registries (confirmed present on a build with NONE of this
// PR's changes) that is out of this PR's scope.

function minimal(): void {
  const baseCap = "base-capture";
  const subCap = "sub-capture";
  const Base1 = class {
    m() {
      return baseCap;
    }
  };
  const Sub1 = class extends Base1 {
    n() {
      return subCap;
    }
  };
  console.log("minimal", new Base1().m(), new Sub1().m(), new Sub1().n());
}
minimal();

// Control: an explicit `constructor() { super(); }` on the subclass must
// keep working (pre-existing "Works" case; a naive union of parent
// captures into every subclass regressed exactly this shape during
// development of this fix — kept here as the regression guard).
function explicitCtor(): void {
  const cap2 = "explicit-super-cap";
  const Base2 = class {
    m() {
      return cap2;
    }
  };
  const Sub2 = class extends Base2 {
    constructor() {
      super();
    }
  };
  console.log("explicitCtor", new Sub2().m());
}
explicitCtor();

// A captured HELPER function (not just a string) read from an inherited
// method on a subclass instance; subclass has its own (uncaptured) member.
function capturedHelper(): void {
  function helper3(x: string): string {
    return "[" + x + "]";
  }
  const Base3 = class {
    m() {
      return helper3("base");
    }
  };
  const Sub3 = class extends Base3 {
    own() {
      return "own";
    }
  };
  console.log("capturedHelper", new Sub3().m(), new Sub3().own());
}
capturedHelper();
