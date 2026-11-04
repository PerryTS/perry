import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const { Certificate } = require("crypto");
console.log(typeof Certificate, Certificate.name, Certificate.length);
console.log(JSON.stringify(Reflect.ownKeys(Certificate)));
const d: any = Object.getOwnPropertyDescriptor(Certificate, "prototype");
console.log(d ? [typeof d.value, d.writable, d.enumerable, d.configurable].join() : String(d));
if (d) {
  const p = d.value;
  console.log(JSON.stringify(Reflect.ownKeys(p)), p.constructor === Certificate, Object.getPrototypeOf(p) === Object.prototype);
  console.log(JSON.stringify(Object.getOwnPropertyNames(p).map((k: string) => { const e: any = Object.getOwnPropertyDescriptor(p, k); return [k, typeof e.value, e.writable, e.enumerable, e.configurable]; })));
  console.log(Certificate.prototype === p);
}
console.log(typeof Certificate.verifySpkac);
