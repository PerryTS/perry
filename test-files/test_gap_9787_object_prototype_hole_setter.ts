// Object.prototype alone must invalidate the numeric hole-store fast path.
"use strict";

let calls = 0;
let receiverIsArray = false;
let assigned = 0;
Object.defineProperty(Object.prototype, "8", {
  configurable: true,
  get() { return "object-proto-eight"; },
  set(this: any, value: number) {
    calls++;
    receiverIsArray = Array.isArray(this);
    assigned = value;
  },
});

let setterResult = "";
try {
  const holes: any[] = new Array(10);
  holes[8] = 53;
  setterResult = [calls, receiverIsArray, assigned, Object.hasOwn(holes, 8), holes[8], holes.length].join(" ");
} finally {
  delete (Object.prototype as any)[8];
}
console.log(setterResult);

Object.defineProperty(Object.prototype, "8", {
  configurable: true,
  get() { return "getter-only"; },
});
let getterResult = "";
try {
  const holes: any[] = new Array(10);
  let error = "none";
  try { holes[8] = 59; } catch (e) { error = (e as Error).constructor.name; }
  getterResult = [error, Object.hasOwn(holes, 8), holes[8], holes.length].join(" ");
} finally {
  delete (Object.prototype as any)[8];
}
console.log(getterResult);

Object.defineProperty(Object.prototype, "8", {
  configurable: true, value: "locked", writable: false,
});
let lockedResult = "";
try {
  const holes: any[] = new Array(10);
  let error = "none";
  try { holes[8] = 61; } catch (e) { error = (e as Error).constructor.name; }
  lockedResult = [error, Object.hasOwn(holes, 8), holes[8], holes.length].join(" ");
} finally {
  delete (Object.prototype as any)[8];
}
console.log(lockedResult);
