// A global the realm declares (`JSON`, `Math`), replaced by the program on
// the global object: a read of it as a value, bare or through `globalThis`,
// runs what was stored, and the objects it is handed are changed by it.
class P {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
const seen: P[] = [];
const g = globalThis as any;
const realJSON = JSON;
g.JSON = {
  stringify(v: any) {
    seen.push(v);
    v.x = v.x + 10;
    return "replaced";
  },
  parse: realJSON.parse,
};
const p = new P(1);
const J: any = JSON;
console.log(J.stringify(p), p.x);
const K: any = g.JSON;
console.log(K.stringify(p), p.x, seen.length, seen[0] === p);
g.JSON = realJSON;
const R: any = JSON;
console.log(R.stringify({ a: p.x }));
g.Math = {
  max(a: number, b: number) {
    return a - b;
  },
};
const M: any = Math;
console.log(M.max(5, 3));
