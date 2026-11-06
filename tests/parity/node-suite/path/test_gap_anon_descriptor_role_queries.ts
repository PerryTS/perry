import * as path from "node:path";
import pathDefault from "node:path";
import { inspect } from "node:util";
const n = Number(process.argv[2] || 4);
const record: any = { x: 1 };
const thenProto: any = { then(resolve: any) { resolve(23); } };
const thenable: any = Object.create(thenProto);
class Declared {
  value = 31;
  then(resolve: any) { resolve(this.value); }
  [Symbol.iterator]() { return [this.value][Symbol.iterator](); }
}
const declared = new Declared();
const sym = Symbol.for("descriptor-role-check");
const inherited: any = Object.create({ [sym]: 19, inherited: 17 });
console.log("declared", "value" in declared, "then" in declared, typeof Reflect.get(declared, Symbol.iterator));
console.log("inherited", "inherited" in inherited, Reflect.get(inherited, sym));
console.log("ordinary", "toString" in record, "constructor" in record);
let checksum = 0;
let resolved = 0;
let nativeResolved = 0;
let classResolved = 0;
const jobs: Promise<any>[] = [];
function missing(obj: any, key: string): boolean { return key in obj; }
for (let i = 0; i < n; i++) {
  if (!missing(i % 2 === 0 ? record : path, "absent" + (i % 3))) checksum++;
  if (Reflect.get(path, Symbol.iterator) === undefined) checksum++;
  if (Object.getPrototypeOf(pathDefault) === Object.prototype) checksum++;
  if (inspect(record) === "{ x: 1 }") checksum++;
  jobs.push(Promise.resolve(thenable).then(value => { resolved += value; }));
  jobs.push(Promise.resolve(path).then(value => { if (value === path) nativeResolved++; }));
  jobs.push(Promise.resolve(declared).then(value => { classResolved += Number(value); }));
}
console.log("checks", checksum);
Promise.all(jobs).then(() => console.log("resolved", resolved, nativeResolved, classResolved));
