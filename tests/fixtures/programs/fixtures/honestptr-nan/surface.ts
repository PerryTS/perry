import addon from "fixture-honestptr-nan";
const value: any = addon.negativeNaN();
const array: any[] = [value];
const object: any = { value };
if (typeof globalThis.gc === "function") globalThis.gc();
console.log("napi", typeof array[0], Number.isNaN(array[0]), typeof object.value, Number.isNaN(object.value));
console.log("math", typeof Math.abs(value), Number.isNaN(Math.abs(value)));
console.log("clone", Number.isNaN(structuredClone(object).value));
