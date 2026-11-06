import { promisify } from "node:util";

console.log("custom symbol:", promisify.custom === Symbol.for("nodejs.util.promisify.custom"));
const obj = {
  value: 5,
  add(x: number, cb: Function) { cb(null, this.value + x); },
};
console.log("bound:", await promisify(obj.add).call(obj, 7));
function multi(cb: Function) { cb(null, "a", "b"); }
console.log("multi:", await promisify(multi)());
function custom(cb: Function) { cb(null, "unused"); }
(custom as any)[promisify.custom] = () => Promise.resolve("custom-result");
console.log("custom:", await promisify(custom)());
