// A descriptor at the END of an array's chain (not its direct prototype) must
// invalidate the numeric hole-store fast path. DEC 250 (Q2): a compiled
// program adds no index key to Object.prototype, so the end of the chain is a
// plain object above the array's own prototype, as Object.prototype was above
// Array.prototype.
"use strict";

const base: any = {};
const mid: any = Object.create(base);
function holesArray(): any[] {
  const holes: any[] = new Array(10);
  Object.setPrototypeOf(holes, mid);
  return holes;
}

let calls = 0;
let receiverIsArray = false;
let assigned = 0;
Object.defineProperty(base, "8", {
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
  const holes: any[] = holesArray();
  holes[8] = 53;
  setterResult = [calls, receiverIsArray, assigned, Object.hasOwn(holes, 8), holes[8], holes.length].join(" ");
} finally {
  delete base[8];
}
console.log(setterResult);

Object.defineProperty(base, "8", {
  configurable: true,
  get() { return "getter-only"; },
});
let getterResult = "";
try {
  const holes: any[] = holesArray();
  let error = "none";
  try { holes[8] = 59; } catch (e) { error = (e as Error).constructor.name; }
  getterResult = [error, Object.hasOwn(holes, 8), holes[8], holes.length].join(" ");
} finally {
  delete base[8];
}
console.log(getterResult);

Object.defineProperty(base, "8", {
  configurable: true, value: "locked", writable: false,
});
let lockedResult = "";
try {
  const holes: any[] = holesArray();
  let error = "none";
  try { holes[8] = 61; } catch (e) { error = (e as Error).constructor.name; }
  lockedResult = [error, Object.hasOwn(holes, 8), holes[8], holes.length].join(" ");
} finally {
  delete base[8];
}
console.log(lockedResult);
