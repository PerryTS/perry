// A callback called inside a loop is resolved once to its code pointer and
// entered with an inline indirect call; the call's type must match the body's
// (wasm traps on a mismatch).
function viaFor(xs, f) { for (let i = 0; i < xs.length; i++) f(xs[i]); }
function viaForOf(xs, f) { for (const x of xs) f(x); }
function viaWhile(n, f) { let i = 0; let s = 0; while (i < n) { s += f(i, s); i++; } return s; }
const seen = [];
viaFor(['a', 'b'], (k) => seen.push('for:' + k));
viaForOf(['c', 'd'], function (k) { seen.push('of:' + k); });
console.log(seen.join(','));
console.log(viaWhile(5, (i, s) => i + s));
const suffix = '!';
const rest = (...a) => a.length + suffix;
function viaRest(xs, f) { const out = []; for (let i = 0; i < xs.length; i++) out.push(f(xs[i], i, 3)); return out; }
console.log(viaRest([1, 2], rest).join(','));
class Box {
  constructor() { this.n = 0; }
  bump(d) { this.n += d; return this.n; }
}
const boxes = [new Box(), new Box()];
let total = 0;
for (let i = 0; i < 4; i++) total += boxes[i % 2].bump(i);
console.log(total);
