// A null [[Prototype]] is the object's shape identity: born null and relinked,
// or linked to null later, every reader agrees with the link.
import { inspect } from "node:util";
import * as ns from "node:path";
const o: any = Object.setPrototypeOf(Object.create(null), Object.prototype);
o.a = 1;
console.log("1", typeof o.hasOwnProperty, o.hasOwnProperty("a"), String(o), `${o}`, o + "");
console.log("2", Object.getPrototypeOf(o) === Object.prototype, o instanceof Object, Object.prototype.isPrototypeOf(o));
console.log("3", inspect(o), inspect(Object.create(null)));
const p = { inherited: "yes", toString() { return "P!"; } };
const q: any = Object.setPrototypeOf(Object.create(null), p);
q.own = 2;
const keys: string[] = [];
for (const k in q) keys.push(k);
console.log("4", keys.join(","), q.inherited, String(q), JSON.stringify(Object.assign({}, q)));
const r: any = Object.setPrototypeOf({ x: 1 }, null);
console.log("5", Object.getPrototypeOf(r), typeof r.toString, inspect(r), "toString" in r);
let threw = false;
try { String(r); } catch (e) { threw = e instanceof TypeError; }
console.log("6", threw);
const n: any = Object.create(null);
n.k = 1;
console.log("7", Object.getPrototypeOf(n), typeof n.toString, inspect(n), n instanceof Object);
class K { k = 1; }
const kk: any = Object.setPrototypeOf(Object.create(null), K.prototype);
console.log("8", kk instanceof K, Object.getPrototypeOf(kk) === K.prototype, typeof kk.constructor);
console.log("9", Object.getPrototypeOf(ns), typeof (ns as any).toString);
const back: any = Object.setPrototypeOf(Object.setPrototypeOf(Object.create(null), Object.prototype), null);
console.log("10", Object.getPrototypeOf(back), typeof back.toString, inspect(back));
const thenable: any = Object.setPrototypeOf(Object.create(null), { then(res: any) { res("thenable"); } });
Promise.resolve(thenable).then((v) => console.log("11", v));
const plainThen: any = Object.create(null);
plainThen.then = (res: any) => res("own then");
Promise.resolve(plainThen).then((v) => console.log("12", v));
