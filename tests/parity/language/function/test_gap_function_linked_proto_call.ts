// A function whose [[Prototype]] was replaced (Object.setPrototypeOf) takes
// call/apply/bind from its new chain, at every kind of call site.
function t(this: any, n: number) {
  return "own " + n;
}
const p = {
  call(this: any, x: any, n: number) {
    return "p.call " + (this === t) + " " + n;
  },
  apply() {
    return "p.apply";
  },
  bind() {
    return "p.bind";
  },
};
Object.setPrototypeOf(t, p);
console.log("direct", (t as any).call(null, 7), (t as any).apply(), (t as any).bind());
const box: any[] = [t];
const g: any = box[(Date.now() & 0) as number];
console.log("opaque", g.call(null, 8), g.apply(), g.bind());
function site(f: any, n: number): any {
  return f.call(null, n);
}
console.log("site", site(t, 9));

// A prototype without the method: inherited from Object.prototype -> missing.
function u() {
  return 1;
}
Object.setPrototypeOf(u, { other: 1 });
try {
  (u as any).call(null);
  console.log("missing no throw");
} catch (e: any) {
  console.log("missing", e instanceof TypeError);
}

// A function prototype: Function.prototype's builtins still apply.
function base() {
  return "base";
}
function w(this: any, a: number) {
  return "w " + a;
}
Object.setPrototypeOf(w, base);
console.log("fn-proto", w.call(null, 1), w.apply(null, [2]), w.bind(null, 3)());

// Restored to Function.prototype: the builtin again.
Object.setPrototypeOf(t, Function.prototype);
console.log("restored", t.call(null, 4), site(t, 5));
