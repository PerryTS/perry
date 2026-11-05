// Issue #1788: a class DECLARATION that `extends` a class-expression value
// (`class Sub extends make("A") {}`) must inherit the parent class object's
// per-evaluation OWN static fields. effect's base schemas use exactly this
// shape: `class Number$ extends make(numberKeyword) {}`, then read
// `Number$.ast`. The parent class object is recorded as the subclass's
// static prototype at `extends` time; static-field reads walk that chain
// (also covering a multi-level `class Leaf extends Mid {}`).
//
// Scope: static FIELD inheritance (the primary criterion). Inherited static
// METHODS through a dynamic parent are tracked as the remaining #1788
// sub-item.
//
// Expected output:
// Sub.ast: A
// Other.ast: B
// Leaf.ast (2-level): M

function make(tag: string) {
  return class {
    static ast = tag;
  };
}

class Sub extends make("A") {}
console.log("Sub.ast:", (Sub as any).ast);

// A second subclass off a distinct evaluation gets the distinct parent field.
class Other extends make("B") {}
console.log("Other.ast:", (Other as any).ast);

// Two-level: Leaf inherits from Mid which extends the class object.
class Mid extends make("M") {}
class Leaf extends Mid {}
console.log("Leaf.ast (2-level):", (Leaf as any).ast);
