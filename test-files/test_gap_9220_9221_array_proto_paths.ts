// #9220 / #9221: the array-index write and borrowed Array.prototype method
// paths must use the same recorded-[[Prototype]] classification as direct
// indexed reads. These were silent wrong answers: assignments created an own
// element instead of invoking/rejecting through an inherited descriptor, and
// the generic array-like engine treated a prototype-filled hole as absent.
//
// The array-prototype cases are controls: both bugs pre-date support for a
// non-array custom prototype and reproduce when the prototype is itself an
// array, a shape Perry has long supported.

const hasOwn = (value: any, key: PropertyKey) =>
  Object.prototype.hasOwnProperty.call(value, key);

// --- #9220: inherited accessor writes -------------------------------------

const writeCalls: any[] = [];
const writeTarget: any = [1, 2, 3];
let writeThis = false;
const writeProto: any = {};
Object.defineProperty(writeProto, "9", {
  configurable: true,
  get() {
    return "acc9";
  },
  set(this: any, value: any) {
    writeCalls.push(value);
    writeThis = this === writeTarget;
  },
});
Object.setPrototypeOf(writeTarget, writeProto);
writeTarget[9] = 5;
console.log(
  "write object accessor:",
  writeCalls.join(","),
  writeThis,
  hasOwn(writeTarget, 9),
  writeTarget[9],
  writeTarget.length,
);

// Exercise the in-bounds-hole shape as well as the out-of-bounds shape above.
const holeWriteCalls: any[] = [];
const holeWriteTarget: any = [0, , 2];
const holeWriteProto: any = {};
Object.defineProperty(holeWriteProto, "1", {
  configurable: true,
  get() {
    return "acc1";
  },
  set(value: any) {
    holeWriteCalls.push(value);
  },
});
Object.setPrototypeOf(holeWriteTarget, holeWriteProto);
holeWriteTarget[1] = 7;
console.log(
  "write hole accessor:",
  holeWriteCalls.join(","),
  hasOwn(holeWriteTarget, 1),
  holeWriteTarget[1],
  holeWriteTarget.length,
);

// Control: the same inherited-setter bug with an Array as [[Prototype]].
const arrayWriteCalls: any[] = [];
const arrayWriteTarget: any = [1];
const arrayWriteProto: any = [];
Object.defineProperty(arrayWriteProto, "4", {
  configurable: true,
  get() {
    return "arrayAcc4";
  },
  set(value: any) {
    arrayWriteCalls.push(value);
  },
});
Object.setPrototypeOf(arrayWriteTarget, arrayWriteProto);
arrayWriteTarget[4] = 11;
console.log(
  "write array accessor:",
  arrayWriteCalls.join(","),
  hasOwn(arrayWriteTarget, 4),
  arrayWriteTarget[4],
  arrayWriteTarget.length,
);

// An inherited writable data property does not intercept OrdinarySet: the
// receiver gets an own property and the prototype value is unchanged.
const dataProto: any = { 5: "protoFive" };
const dataTarget: any = [1];
Object.setPrototypeOf(dataTarget, dataProto);
dataTarget[5] = "ownFive";
console.log(
  "write inherited data:",
  hasOwn(dataTarget, 5),
  dataTarget[5],
  dataProto[5],
  dataTarget.length,
);

// A non-writable inherited data property rejects the assignment. This file is
// an ES module (the repository package is type=module), so rejection throws.
const lockedProto: any = {};
Object.defineProperty(lockedProto, "6", {
  configurable: true,
  enumerable: true,
  value: "lockedSix",
  writable: false,
});
const lockedTarget: any = [1];
Object.setPrototypeOf(lockedTarget, lockedProto);
let lockedThrew = false;
try {
  lockedTarget[6] = "changed";
} catch {
  lockedThrew = true;
}
console.log(
  "write inherited readonly:",
  lockedThrew,
  hasOwn(lockedTarget, 6),
  lockedTarget[6],
  lockedTarget.length,
);

// --- #9221: borrowed Array.prototype methods over a real Array ------------

const genericProto: any = { 1: "holeFill" };
const genericTarget: any = [0, , 2];
Object.setPrototypeOf(genericTarget, genericProto);
const genericMapSeen: string[] = [];
const genericMapped: any = Array.prototype.map.call(
  genericTarget,
  (value: any, index: number) => {
    genericMapSeen.push(index + ":" + value);
    return String(value).toUpperCase();
  },
);
const genericEachSeen: string[] = [];
Array.prototype.forEach.call(genericTarget, (value: any, index: number) => {
  genericEachSeen.push(index + ":" + value);
});
console.log("generic object join:", Array.prototype.join.call(genericTarget));
console.log(
  "generic object indexOf:",
  Array.prototype.indexOf.call(genericTarget, "holeFill"),
);
console.log(
  "generic object map:",
  genericMapSeen.join("|"),
  Array.prototype.join.call(genericMapped, "|"),
  hasOwn(genericMapped, 1),
);
console.log("generic object forEach:", genericEachSeen.join("|"));

// Accessor fill: Get must bind the original array as receiver.
let genericGetterCalls = 0;
let genericGetterThis = true;
const genericGetterTarget: any = [0, , 2];
const genericGetterProto: any = {};
Object.defineProperty(genericGetterProto, "1", {
  configurable: true,
  get(this: any) {
    genericGetterCalls++;
    genericGetterThis = genericGetterThis && this === genericGetterTarget;
    return "getterFill";
  },
});
Object.setPrototypeOf(genericGetterTarget, genericGetterProto);
const getterJoined = Array.prototype.join.call(genericGetterTarget, "-");
console.log(
  "generic getter join:",
  getterJoined,
  genericGetterCalls,
  genericGetterThis,
);

// Control: identical generic-engine divergence with an Array [[Prototype]].
const genericArrayProto: any = [];
genericArrayProto[1] = "arrayFill";
const genericArrayTarget: any = [0, , 2];
Object.setPrototypeOf(genericArrayTarget, genericArrayProto);
const genericArraySeen: string[] = [];
Array.prototype.forEach.call(genericArrayTarget, (value: any, index: number) => {
  genericArraySeen.push(index + ":" + value);
});
console.log(
  "generic array control:",
  Array.prototype.join.call(genericArrayTarget),
  Array.prototype.indexOf.call(genericArrayTarget, "arrayFill"),
  genericArraySeen.join("|"),
);

// Default-chain control: ordinary holes remain holes for HasProperty methods.
const defaultTarget: any = [0, , 2];
const defaultSeen: string[] = [];
Array.prototype.forEach.call(defaultTarget, (value: any, index: number) => {
  defaultSeen.push(index + ":" + value);
});
console.log(
  "generic default control:",
  Array.prototype.join.call(defaultTarget),
  Array.prototype.indexOf.call(defaultTarget, undefined),
  defaultSeen.join("|"),
);
