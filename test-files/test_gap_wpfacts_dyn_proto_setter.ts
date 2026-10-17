// Decision 150: an interpreted function receives an object and installs a
// setter and getter under `v` on that object's prototype, which is
// `Object.prototype`. A constructor that assigns `this.v = …` afterwards
// runs the setter instead of defining `v`, so `Cell`'s birth field is
// missing on later instances. `rv`'s receiver holds only `Cell` instances
// (nothing of it reaches interpreted code), yet its read must not assume
// the birth slot: the proof is guarded by the runtime's
// interpreted-prototype-write word and takes the generic read.
class Cell {
  // `declare`: no field definition, so the constructor's assignments
  // are what create `v` and `w` (an inherited setter intercepts them).
  declare v: number;
  declare w: number;
  constructor(v: number) {
    this.v = v;
    this.w = v + 1;
  }
}

function rv(c: Cell): any {
  return c.v;
}

function rw(c: Cell): any {
  return c.w;
}

const before = new Cell(1);
console.log(rv(before), rw(before));
const escaped: any = { tag: 1 };
const install = new Function(
  "o",
  [
    "const P = Object.getPrototypeOf(o);",
    "Object.defineProperty(P, 'v', {",
    "  set(x) { Object.defineProperty(this, 'shadow', { value: x, enumerable: true, configurable: true }); },",
    "  get() { return -1; },",
    "  configurable: true,",
    "});",
  ].join("\n"),
);
install(escaped);
const after = new Cell(7);
console.log(rv(after), rw(after), Object.keys(after).join(","));
console.log(rv(before), rw(before));
