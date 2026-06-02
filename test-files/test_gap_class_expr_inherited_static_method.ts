// Issue #1788 (follow-up): a class DECLARATION that `extends` a
// class-expression value inherits the parent's STATIC METHODS, not just
// static fields. effect's base schemas do this: `class Number$ extends
// make(numberKeyword) {}`, then `Number$.pipe(...)` / `Number$.annotations(...)`
// dispatch to the inherited static method with `this` = the subclass.
//
// Static methods are emitted as `perry_static_*` (no `this` param) and live
// in the runtime CLASS_STATIC_METHODS table keyed by the template class_id;
// `Sub.greet()` walks Sub's class_id parent chain to find it and binds `this`
// to Sub so `this.<field>` resolves through the subclass's static-field chain.
//
// Scope note: fixed-arity inherited static methods. Rest-param static methods
// (`static pipe(...args)`) need arg-array bundling and are a tracked
// refinement.
//
// Expected output:
// Sub.greet: hi-A
// Sub.withArgs: x+y:A
// Leaf.greet (2-level): hi-M

function make(tag: string) {
  return class {
    static ast = tag;
    static greet() {
      return "hi-" + this.ast;
    }
    static withArgs(a: string, b: string) {
      return a + "+" + b + ":" + this.ast;
    }
  };
}

class Sub extends make("A") {}
console.log("Sub.greet:", (Sub as any).greet());
console.log("Sub.withArgs:", (Sub as any).withArgs("x", "y"));

class Mid extends make("M") {}
class Leaf extends Mid {}
console.log("Leaf.greet (2-level):", (Leaf as any).greet());
