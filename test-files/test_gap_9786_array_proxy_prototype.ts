// Array fast paths must walk the *actual* full prototype chain and must not
// treat Proxy prototypes as if no custom prototype existed.

"use strict";

const setterLog: string[] = [];
const grand: any = {};
Object.defineProperty(grand, "3", {
  configurable: true,
  get() {
    return "from-grand";
  },
  set(this: any, value: any) {
    setterLog.push(`${this === deep}:${value}`);
  },
});
const middle: any[] = [];
Object.setPrototypeOf(middle, grand);
const deep: any[] = [0];
Object.setPrototypeOf(deep, middle);
deep[3] = 17;
console.log(setterLog.join(","), Object.hasOwn(deep, 3), deep[3], deep.length);

const proxyLog: string[] = [];
let proxied: any[];
const proxyPrototype = new Proxy(
  {},
  {
    get(_target, key, receiver) {
      if (key === "4") proxyLog.push(`get:${receiver === proxied}`);
      return key === "4" ? "proxy-four" : Reflect.get(_target, key, receiver);
    },
    has(_target, key) {
      if (key === "4") proxyLog.push("has");
      return key === "4" || Reflect.has(_target, key);
    },
    set(_target, key, value, receiver) {
      proxyLog.push(`set:${String(key)}:${value}:${receiver === proxied}`);
      return true;
    },
  },
);
proxied = [1];
Object.setPrototypeOf(proxied, proxyPrototype);
proxied[4] = 29;
console.log(Object.hasOwn(proxied, 4), proxied[4], 4 in proxied, proxied.length);
console.log(proxyLog.join("|"));

const holeGrand: any = { 1: "inherited-hole" };
const holeMiddle: any[] = [];
Object.setPrototypeOf(holeMiddle, holeGrand);
const holey: any[] = [0, , 2];
Object.setPrototypeOf(holey, holeMiddle);
const seen: string[] = [];
Array.prototype.forEach.call(holey, (v: any, i: number) => seen.push(`${i}:${v}`));
console.log(
  Array.prototype.join.call(holey, ","),
  Array.prototype.indexOf.call(holey, "inherited-hole"),
  seen.join("|"),
);
