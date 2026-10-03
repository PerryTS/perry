// #11759 (c′): code after a repeatable class declaration is lowered twice,
// once for its first evaluation (the shared class) and once for later ones.
// Both versions must behave identically: numeric fields, captured values,
// methods, instanceof, and an instance created before a loop.
function make(k: number, n: number) {
  class C {
    x: number;
    y = k * 2;
    constructor(x: number) { this.x = x + k; }
    m() { return this.x + this.y; }
    static s = k;
  }
  const c0 = new C(1);
  let t = 0;
  for (let i = 0; i < n; i++) {
    const c = new C(i);
    t += c.x + c.y;
    t += c0.m();
  }
  for (let i = 0; i < n; i++) { t += C.s; }
  return [t, c0.m(), c0 instanceof C, C.s, typeof c0.x];
}
console.log(JSON.stringify(make(1, 3)));
console.log(JSON.stringify(make(2, 3)));
console.log(JSON.stringify(make(0.5, 4)));

// A field that holds a string on every evaluation stays a string.
function strings(tag: string) {
  class S { v: string; constructor(v: string) { this.v = v + tag; } }
  let out = "";
  for (let i = 0; i < 3; i++) { const s = new S(String(i)); out += s.v; }
  const s0 = new S("!");
  for (let i = 0; i < 2; i++) { out += s0.v; }
  return out;
}
console.log(strings("a"));
console.log(strings("b"));

// The binding is rebound in the tail: no versioning, the per-use test stays.
function rebinding(k: number) {
  let K = class { v = 1; };
  class D { w: number; constructor() { this.w = k; } }
  const d = new D();
  let r = d.w;
  if (k > 1) { K = class { v = 2; }; }
  r += new K().v;
  return r;
}
console.log(rebinding(1), rebinding(2), rebinding(3));
