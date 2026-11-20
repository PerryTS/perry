// A store into a built-in function value writes the key it names: that
// function keeps its other keys, and other built-in functions keep theirs
// (rule R2 of whole-program facts). Element and named keys, a replaced
// declared member (`call`), a program field (`cache`) and a store into what
// may be a built-in method (`nodes[k]`).
class Box {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
}

const max: any = Math.max;
const origCall = max.call;
max.call = function (_t: unknown, a: number, b: number) {
  return a + b;
};
console.log(max.call(null, 2, 3), max(2, 3), max.length, max.name);
console.log(typeof max.apply, max.apply(null, [4, 9]));
console.log(Math.min.length, Math.min.name, typeof Math.min.call);

const push: any = [].push;
push.cache = new Box(7);
console.log(push.cache.n, ([] as any).push.cache.n);

const nodes: any = [new Box(1)];
const k = process.argv.length > 99 ? "push" : "0";
const node = nodes[k];
node.jsDoc = [1, 2];
console.log(node.n, node.jsDoc.length, typeof ([] as any).push.jsDoc);

const f: any = Math.abs;
f[0] = new Box(3);
const idx: any = process.argv.length > 99 ? "x" : "0";
console.log(f[idx].n, Math.abs(-4), f.length);

max.call = origCall;
console.log(max.call(null, 1, 8), push.cache.n);
