// A constructor's `this.k = this.k.bind(this)` that continues its run of
// family stores, in a program where another class has its own `bind` method
// (three.js: the WebGPU Renderer's constructor beside PropertyBinding.bind).
// The `.bind(...)` call lowers as a dispatch over the program's `bind`
// methods with Function.prototype.bind as its generic arm; the store's run
// proof once used that arm's "builtin bind ran" value after the dispatch
// merge, which it does not dominate, and LLVM rejected the module.
class A {
  [k: string]: any;
  constructor() {
    this.x = 1;
    this.f = this.f.bind(this);
  }
  f(): number {
    return this.x + 1;
  }
}
class Binding {
  bind(): string {
    return "user bind";
  }
}
class B extends A {
  y = 2;
}
console.log(new Binding().bind());
const b: any = new B();
const g = b.f;
console.log(g(), b.x, b.y, Object.keys(b).join(","));
const c: any = new B();
console.log(c.f === b.f, c.f(), typeof c.f, c instanceof A);
