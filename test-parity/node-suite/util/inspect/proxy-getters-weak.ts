import { inspect } from "node:util";

const obj: any = {};
Object.defineProperty(obj, "value", { get() { return 42; }, enumerable: true });
console.log("getter false:", inspect(obj));
console.log("getter true:", inspect(obj, { getters: true }));
const proxy = new Proxy({ a: 1 }, {});
console.log("proxy default:", inspect(proxy));
console.log("proxy shown:", inspect(proxy, { showProxy: true }));
console.log("weakmap:", inspect(new WeakMap([[{}, 1]]), { showHidden: true }));
