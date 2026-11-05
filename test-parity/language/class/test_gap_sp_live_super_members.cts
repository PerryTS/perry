class Base { m() { return 'base:' + this.value; } static m() { return 'base:' + this.value; } }
class Leaf extends Base {
  value = 4;
  m() { return super.m(); }
  arrow() { return () => super.m(); }
  static value = 5;
  static m() { return super.m(); }
  static arrow() { return () => super.m(); }
}
const leaf = new Leaf();
const captured = leaf.arrow();
const staticCaptured = Leaf.arrow();
console.log(leaf.m(), captured(), Leaf.m(), staticCaptured());
Object.setPrototypeOf(Leaf.prototype, { m() { return 'other:' + this.value; } });
Object.setPrototypeOf(Leaf, { m() { return 'other:' + this.value; } });
console.log(leaf.m(), captured(), Leaf.m(), staticCaptured());
