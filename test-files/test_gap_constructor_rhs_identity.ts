// parity-node-argv: --expose-gc
"use strict";
export {};
declare function gc(): void;
const SavedDate: any = Date;
const SavedNumber: any = Number;
const SavedObject: any = Object;
const SavedArray: any = Array;
const dateChild: any = Object.create(Date.prototype);
const numberChild: any = Object.create(Number.prototype);
function dateAlias(value: any): boolean { return value instanceof SavedDate; }
function liveDate(value: any): boolean { return value instanceof Date; }
console.log("initial", dateAlias(dateChild), liveDate(dateChild));
const replacement: any = function Replacement() {};
(globalThis as any).Date = replacement;
console.log("date alias", dateAlias(dateChild), liveDate(dateChild));
(globalThis as any).Date = SavedDate;
let hookReads = 0;
Object.defineProperty(SavedDate, Symbol.hasInstance, {
  configurable: true,
  get() { hookReads++; if (typeof gc === "function") gc(); return undefined; }
});
console.log("date getter", dateAlias(dateChild), hookReads);
delete SavedDate[Symbol.hasInstance];
(globalThis as any).Number = replacement;
console.log("number alias", numberChild instanceof SavedNumber, numberChild instanceof Number);
(globalThis as any).Number = SavedNumber;
// Constructor and static-property reads must preserve real receiver semantics.
const object: any = {};
const array: any = [];
console.log("constructors", object.constructor === SavedObject, array.constructor === SavedArray);
function arrayConstructor(value: any): any { return value.constructor; }
for (let i = 0; i < 20; i++) {
  if (arrayConstructor(array) !== SavedArray) throw new Error("initial array constructor");
}
(globalThis as any).Array = replacement;
console.log("array binding replaced", arrayConstructor(array) === SavedArray);
(globalThis as any).Array = SavedArray;
const arrayDescriptor: any = Object.getOwnPropertyDescriptor(SavedArray.prototype, "constructor");
let arrayCalls = 0;
Object.defineProperty(SavedArray.prototype, "constructor", {
  configurable: true,
  get() { arrayCalls++; return this === array ? replacement : undefined; }
});
console.log("array getter", arrayConstructor(array) === replacement, arrayCalls);
delete SavedArray.prototype.constructor;
console.log("array constructor deleted", arrayConstructor(array) === SavedObject);
Object.defineProperty(SavedArray.prototype, "constructor", arrayDescriptor);
const fn: any = function Probe() {};
console.log("absent", fn.isBuffer === undefined);
let calls = 0;
Object.defineProperty(Function.prototype, "isBuffer", {
  configurable: true,
  get() { calls++; return this === fn ? replacement : undefined; }
});
console.log("inherited getter", fn.isBuffer === replacement, calls);
Object.defineProperty(fn, "isBuffer", { configurable: true, value: 19 });
console.log("own", fn.isBuffer, calls);
delete fn.isBuffer;
console.log("own deleted", fn.isBuffer === replacement, calls);
delete (Function.prototype as any).isBuffer;
console.log("prototype deleted", fn.isBuffer === undefined, calls);
// A terminal shape may answer the walk only after the actual RHS prototype
// has been read, and only until the receiver's prototype changes.
const terminal: any = {};
const nil: any = Object.create(null);
const SavedMap: any = Map;
console.log("terminal native", terminal instanceof SavedMap, nil instanceof SavedMap);
const Ordinary: any = function Ordinary() {};
Ordinary.prototype = Object.prototype;
console.log("terminal target", terminal instanceof Ordinary, nil instanceof Ordinary);
Object.setPrototypeOf(terminal, SavedMap.prototype);
console.log("physical target", terminal instanceof SavedMap, terminal instanceof Ordinary);
