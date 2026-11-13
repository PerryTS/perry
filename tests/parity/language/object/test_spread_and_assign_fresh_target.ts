// Spread and Object.assign into a fresh target: the result is born wide
// enough for the copied keys; key order, attributes and getter timing are
// unchanged.

function show(label: string, o: any): void {
  const keys = Reflect.ownKeys(o).map(String);
  const first = keys.length > 0 ? keys[0] : "";
  const last = keys.length > 0 ? keys[keys.length - 1] : "";
  console.log(
    label,
    JSON.stringify(Object.keys(o)),
    JSON.stringify(o),
    JSON.stringify(keys),
    JSON.stringify(Object.getOwnPropertyDescriptor(o, first) ?? null),
    JSON.stringify(Object.getOwnPropertyDescriptor(o, last) ?? null),
  );
  o.addedAfter = 1;
  console.log(label, "after add", JSON.stringify(Object.keys(o)), o.addedAfter);
}

function make(n: number): any {
  const o: any = {};
  for (let i = 0; i < n; i++) o["k" + i] = i;
  return o;
}

for (const n of [0, 1, 2, 3, 8, 40]) {
  const src = make(n);
  show("spread" + n, { ...src });
  show("spreadTail" + n, { ...src, tail: n });
  show("spreadHead" + n, { head: 1, ...src, tail: 2 });
  show("assign" + n, Object.assign({}, src));
}

class Point {
  x = 1;
  y = 2;
  z: number;
  constructor(z: number) { this.z = z; }
  get sum(): number { return this.x + this.y; }
}
show("classSpread", { ...new Point(3), w: 4 });
show("classAssign", Object.assign({}, new Point(3)));

const log: string[] = [];
const grow: any = { a: 1 };
Object.defineProperty(grow, "g", {
  enumerable: true,
  configurable: true,
  get() { log.push("get g"); grow.added = 5; return 7; },
});
grow.b = 2;
show("getterSpread", { ...grow, after: 1 });
show("getterAssign", Object.assign({}, grow));
console.log(log.join(","), JSON.stringify(Object.keys(grow)));

const rep: any = { a: 1, b: 2, c: 3 };
show("repeatBefore", { b: 0, ...rep, d: 4 });
show("repeatAfter", { ...rep, a: 9, e: 5 });
show("repeatBoth", { c: 0, ...rep, c2: 1, c: 7 } as any);

const sym = Symbol("s");
const hidden = Symbol("h");
const withSym: any = { a: 1, [sym]: 2 };
Object.defineProperty(withSym, hidden, { value: 3, enumerable: false });
const spreadSym = { ...withSym, z: 1 };
const assignSym = Object.assign({}, withSym);
show("symSpread", spreadSym);
show("symAssign", assignSym);
console.log(spreadSym[sym], assignSym[sym], spreadSym[hidden], assignSym[hidden]);

const others: any[] = [[1, 2, 3], "ab", null, undefined, 42];
for (let i = 0; i < others.length; i++) {
  show("otherSpread" + i, { ...others[i], t: 1 });
  show("otherAssign" + i, Object.assign({}, others[i]));
}

const trapped: string[] = [];
const proxy = new Proxy({ p: 1, q: 2 } as any, {
  ownKeys(t) { trapped.push("ownKeys"); return Reflect.ownKeys(t); },
});
show("proxySpread", { ...proxy, r: 3 });
show("proxyAssign", Object.assign({}, proxy));
console.log(trapped.join(","));

const inner = { i: 1, j: 2 };
show("nested", { ...{ ...inner, k: 3 }, l: 4 });
show("twoSpreads", { ...inner, ...make(3), m: 5 });
show("assignTwo", Object.assign({}, inner, make(3)));
const frozen = Object.freeze({ f: 1, g: 2, h: 3 });
show("frozenSpread", { ...frozen, x: 1 });
const fromFrozen = Object.assign({}, frozen);
show("frozenAssign", fromFrozen);
console.log(Object.isFrozen(fromFrozen));

let total = 0;
const base: any = { a: 1, b: 2, c: 3, d: 4, e: 5 };
for (let i = 0; i < 2000; i++) {
  const s = { ...base, extra: i };
  const t = Object.assign({}, base);
  total += s.extra + t.e + Object.keys(s).length + Object.keys(t).length;
}
console.log("loop", total);
