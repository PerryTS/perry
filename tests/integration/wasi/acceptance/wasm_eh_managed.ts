// Nested Rust callback traps inside a Promise context, followed by rejection.
Promise.resolve().then(() => {
  try { [1].map(() => { [2].map(() => { throw new Error("inner callback caught"); }); }); }
  catch (e: any) { console.log(e.message); }
  throw new Error("outer callback caught");
}).catch((e) => console.log(e.message))
  .then(() => console.log("callback context continued"));
class Base { x: number; constructor(x: number) { this.x = x; } }
class Derived extends Base {
  y = 0;
  constructor(fail: boolean) {
    super(5);
    const read = () => this.x;
    try { if (fail) throw new Error("inside derived"); } catch {}
    this.y = read() + 1;
    if (fail) throw new Error("abandon derived");
  }
}
let sum = 0;
for (let i = 0; i < 1000; i++) {
  try { sum += new Derived(i % 2 === 0).y; } catch {}
}
console.log("derived", sum);
function Target(this: any, fail: boolean) {
  try { if (fail) throw new Error("inside target"); } catch {}
  this.calledWithNew = new.target === Target;
  if (fail) throw new Error("abandon target");
}
for (let i = 0; i < 100; i++) { try { new (Target as any)(true); } catch {} }
console.log("new.target", new (Target as any)(false).calledWithNew);
class Private {
  #x = 7;
  read(fail: boolean) {
    try { if (fail) throw new Error("private"); } catch {}
    return this.#x;
  }
}
const privateValue = new Private();
console.log("private", privateValue.read(true));
const map = new Map([[1, 2], [3, 4]]);
const set = new Set([1, 3]);
let caught = 0;
for (let i = 0; i < 10000; i++) {
  try { map.forEach(() => { set.forEach(() => { throw 19; }); }); }
  catch { caught++; }
}
let total = 0;
map.forEach(v => { total += v; });
set.forEach(v => { total += v; });
console.log("repeated catches", caught, total);
