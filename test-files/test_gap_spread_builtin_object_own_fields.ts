// A spread or Object.assign of a builtin object copies its own enumerable
// properties: JSON.parse values, Object.keys arrays, a Date or a Map (none),
// a source with a getter (the getter runs), in key order.
class V {
  k = 1;
  len(): number {
    return this.k;
  }
}
const obj = new V();
const text = '{"b":{"c":[1,"s",null]},"n":2,"a":"from-json"}';

const cfg: any = JSON.parse(text);
const o1: any = { a: obj, ...cfg };
console.log(Object.keys(o1).join(","), o1.a, o1.n, JSON.stringify(o1.b));

const o2: any = Object.assign({ a: obj, z: 0 }, JSON.parse('{"n":3}'));
console.log(Object.keys(o2).join(","), o2.a.len(), o2.n, o2.z);

const o3: any = { ...JSON.parse("[10,20]") };
console.log(Object.keys(o3).join(","), o3[0] + o3[1]);

const ks: any = { ...Object.keys({ p: 1, q: 2 }) };
console.log(JSON.stringify(ks));

const f = o1.cb;
if (typeof f === "function") f(obj);
console.log(obj.len());

const d: any = { a: obj, ...new Date(0) };
console.log(Object.keys(d).join(","), d.a.len());
const m: any = Object.assign({ a: obj }, new Map([["x", 1]]));
console.log(Object.keys(m).join(","));

let runs = 0;
const g: any = {
  get cb() {
    runs++;
    return (v: V) => v.k + 1;
  },
};
const o4: any = { ...g };
console.log(runs, typeof Object.getOwnPropertyDescriptor(o4, "cb")!.value, o4.cb(obj));

const nested = JSON.parse('{"x":{"y":{"z":5}}}');
const o5: any = Object.assign({}, nested, { w: 1 });
console.log(o5.x.y.z, o5.w, o5.x === nested.x);
