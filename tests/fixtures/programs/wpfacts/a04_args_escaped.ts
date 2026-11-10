// `arguments` escapes to a global: every argument escapes with it, even
// with two objects (no widening involved).
// WPFACTS: fact F3 prefix_stable E0 = 0
// WPFACTS: fact F3 prefix_stable E1 = 0
class E0 { constructor() { this.v = 1; } }
class E1 { constructor() { this.v = 2; } }
function keep(a, b) {
  (globalThis as any).kept = arguments;
  return a.v + b.v;
}
let total = keep(new E0(), new E1());
total += (globalThis as any).kept[1].v;
console.log(total);
