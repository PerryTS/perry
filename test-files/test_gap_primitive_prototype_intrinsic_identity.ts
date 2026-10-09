// parity-node-argv: --expose-gc
"use strict";
export {};
declare function gc(): void;
const savedObject: any = Object;
const savedNumber: any = Number;
const numberProto: any = Number.prototype;
const objectProto: any = Object.prototype;
let getterCalls = 0;
savedObject.defineProperty(numberProto, "laneNumber", {
  configurable: true, get: function () { getterCalls++; return this === 7; },
});
savedObject.defineProperty(objectProto, "laneObject", {
  configurable: true, get: function () { return typeof this; },
});
const fakeNumber: any = function FakeNumber() {};
fakeNumber.prototype.laneNumber = "wrong number prototype";
const fakeObject: any = function FakeObject() {};
savedObject.defineProperty(fakeObject.prototype, "laneObject", { get: function () { return "wrong object prototype"; } });
(globalThis as any).Number = fakeNumber;
(globalThis as any).Object = fakeObject;
if (typeof gc === "function") gc();
// First primitive property query follows the original realm's prototypes.
const value: any = 7;
const own = value.laneNumber;
const inherited = value.laneObject;
const constructor = value.constructor === savedNumber;
(globalThis as any).Number = savedNumber;
(globalThis as any).Object = savedObject;
console.log("intrinsic", own, inherited, constructor, getterCalls);
// Prototype mutation remains observable at the next read.
savedObject.defineProperty(numberProto, "laneNumber", { configurable: true, value: 29 });
if (typeof gc === "function") gc();
console.log("changed", value.laneNumber, getterCalls);
delete numberProto.laneNumber;
delete objectProto.laneObject;
console.log("deleted", value.laneNumber, value.laneObject);
