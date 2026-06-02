// Issue #1758/#321: a derived-class constructor's own TS parameter properties
// must be assigned AFTER the super() call (TS semantics: `this` is unusable
// before super). Perry previously prepended `this.field = param` to the top of
// the constructor body, so for a class that calls super() the derived class's
// own param-props were dropped (left undefined) — e.g. effect's SchemaAST
// `class OptionalType extends Type { constructor(type, readonly isOptional) {
// super(type) } }` lost `this.isOptional`.
//
// Node's --experimental-strip-types can't run parameter properties, so this is
// a perry-only expected-output test.
//
// Expected output:
// base: 1 def
// mid: 2 mid true [9]
// leaf: 3 mid false [1,2] leaf
// withBody: 5 T init-T

class Base {
  constructor(public a: number, readonly b: string = "def") {}
}
const base = new Base(1);
console.log("base:", base.a, base.b);

class Mid extends Base {
  constructor(a: number, public c: boolean, readonly d: number[] = [9]) {
    super(a, "mid");
  }
}
const mid = new Mid(2, true);
console.log("mid:", mid.a, mid.b, mid.c, JSON.stringify(mid.d));

class Leaf extends Mid {
  constructor(public e: string) {
    super(3, false, [1, 2]);
  }
}
const leaf = new Leaf("leaf");
console.log("leaf:", leaf.a, leaf.b, leaf.c, JSON.stringify(leaf.d), leaf.e);

class WithBody extends Base {
  log: string;
  constructor(a: number, readonly tag: string) {
    super(a);
    this.log = "init-" + this.tag;
  }
}
const wb = new WithBody(5, "T");
console.log("withBody:", wb.a, wb.tag, wb.log);
