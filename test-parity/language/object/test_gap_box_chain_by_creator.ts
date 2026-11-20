// A container's chain is its creator's: JSON.parse objects are ordinary
// objects (Object.prototype), its arrays arrays, Object.groupBy results have
// a null prototype, Map.groupBy results are Maps.
class V {
  x = 1;
  y = "v";
  sum(): number {
    return this.x + this.y.length;
  }
}

function sanitize(name: any): string {
  return name.replace(/\s/g, "_").replace(/[.:]/g, "");
}

function tryCall(label: string, f: () => any): void {
  try {
    console.log(label, f());
  } catch (e) {
    console.log(label, e instanceof TypeError ? "TypeError" : "other");
  }
}

const texts = ['{"name":"a b.c","n":3}', '{"name":{"replace":1}}', '{"name":["x"]}', '{"name":7}'];
const vs: V[] = [];
for (const s of texts) {
  const j: any = JSON.parse(s);
  const v = new V();
  j.held = v;
  vs.push(v);
  tryCall("sanitize", () => sanitize(j.name));
  tryCall("own-replace", () => j.replace("a", "b"));
  tryCall("toString", () => String(j.toString()));
  tryCall("hasOwn", () => j.hasOwnProperty("name"));
  tryCall("ctor", () => j.constructor === Object);
}

const J: any = JSON;
const arr: any = J.parse('[1,2,3]');
let total = 0;
arr.forEach((x: number) => {
  total += x;
});
console.log("forEach", total, Array.isArray(arr), arr.map((x: number) => x * 2).join(","));
const nested: any = J.parse('{"a":[{"b":2}]}');
console.log("nested", nested.a.map((o: any) => o.b).join(","), typeof nested.replace);

const revived: any = JSON.parse('{"k":[1,{"m":2}]}', function (this: any, key: string, value: any) {
  if (key === "m") return Array.isArray(this) ? "arr" : "obj";
  return value;
});
console.log("revive", JSON.stringify(revived));

const g: any = Object.groupBy([1, 2, 3], (x: number) => (x > 1 ? "big" : "small"));
console.log("groupBy proto", Object.getPrototypeOf(g) === null, g.big.length, typeof g.toString);
tryCall("groupBy-replace", () => g.replace("x"));
const mg: any = Map.groupBy([1, 2, 3], (x: number) => (x > 1 ? "big" : "small"));
console.log("Map.groupBy", mg instanceof Map, mg.get("big").length, mg.size);

const re = /(?<first>\w+)-(?<second>\w+)/.exec("ab-cd") as any;
console.log("groups", Object.getPrototypeOf(re.groups) === null, re.groups.first, typeof re.groups.hasOwnProperty);

const d: any = Object.getOwnPropertyDescriptor({ q: 1 }, "q");
console.log("descriptor", d.value, typeof d.replace, d.hasOwnProperty("value"));
const fe: any = Object.fromEntries([["p", 1]]);
console.log("fromEntries", fe.p, typeof fe.replace, Object.getPrototypeOf(fe) === Object.prototype);

const given: any = JSON.parse('{"w":1}');
Object.setPrototypeOf(given, { replace(a: any, b: any) { return "given:" + a + b; } });
console.log("given", given.replace("p", "q"));

let s = 0;
for (const v of vs) s += v.sum();
console.log("sum", s);
