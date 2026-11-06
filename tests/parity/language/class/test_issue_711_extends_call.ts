// Issue #711: `class X extends fn(...)` / `class X extends Y.method(...)`
// must walk the parent chain for method dispatch even when the parent
// class_id is only known at runtime (the super-class expression is a
// CallExpr / NewExpr, not a static Ident or Member).
//
// Pre-fix: HIR's `lower_decl.rs` extends-clause arms dropped any
// non-Ident/Member shape silently, leaving `extends_name = None` so
// codegen never emitted `js_register_class_parent`. Method dispatch
// terminated at the child's empty vtable and threw
// `<method> is not a function`.

class Base {
  pipe(label: string): string {
    return "Base.pipe:" + label;
  }
  greet(): string {
    return "Base.greet";
  }
}

// Shape 1: free function call as the super-class expression
function factory(): typeof Base {
  return Base;
}
class Derived extends factory() {}

// Shape 2: method call on an Ident (Effect's `String$.pipe(...)` pattern)
class Mid {
  static makeBase(): typeof Base {
    return Base;
  }
}
class Chained extends Mid.makeBase() {}

// Shape 3: `new` expression returning a class
function newWrapper(): typeof Base {
  return Base;
}
class ViaNew extends newWrapper() {}

// Shape 4: chained extends — parent of parent is also dynamically
// registered. The chain walk must follow the dynamic registrations all
// the way up.
class Pipeable {
  pipeMethod(): string {
    return "Pipeable.pipeMethod";
  }
}
function makePipeable(): typeof Pipeable {
  return Pipeable;
}
class StringRefined extends makePipeable() {
  refined(): string {
    return "StringRefined.refined";
  }
}
function applyOptions(c: typeof StringRefined): typeof StringRefined {
  return c;
}
class WithOptions extends applyOptions(StringRefined) {}

console.log("derived.pipe:", new Derived().pipe("hello"));
console.log("derived.greet:", new Derived().greet());
console.log("chained.pipe:", new Chained().pipe("world"));
console.log("chained.greet:", new Chained().greet());
console.log("via_new.pipe:", new ViaNew().pipe("nye"));
console.log("with_options.pipeMethod:", new WithOptions().pipeMethod());
console.log("with_options.refined:", new WithOptions().refined());
