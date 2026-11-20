"use strict";

function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", String(fn()));
  } catch (error) {
    console.log(label + ":", "throw:" + (error as Error).name);
  }
}

const accessor: any = {};
let events: string[] = [];

Object.defineProperty(accessor, "truthy", {
  configurable: true,
  get() {
    events.push("get:truthy");
    return "kept";
  },
  set(value) {
    events.push("set:truthy=" + value);
  },
});
show("accessor ||= skip", () => accessor.truthy ||= "new");
console.log("accessor ||= events:", events.join("|"));

events = [];
Object.defineProperty(accessor, "defined", {
  configurable: true,
  get() {
    events.push("get:defined");
    return "kept";
  },
  set(value) {
    events.push("set:defined=" + value);
  },
});
show("accessor ??= skip", () => accessor.defined ??= "new");
console.log("accessor ??= events:", events.join("|"));

events = [];
Object.defineProperty(accessor, "falsy", {
  configurable: true,
  get() {
    events.push("get:falsy");
    return 0;
  },
  set(value) {
    events.push("set:falsy=" + value);
  },
});
show("accessor &&= skip", () => accessor.falsy &&= 9);
console.log("accessor &&= events:", events.join("|"));

events = [];
let assignedMissing = "";
Object.defineProperty(accessor, "missing", {
  configurable: true,
  get() {
    events.push("get:missing");
    return undefined;
  },
  set(value) {
    assignedMissing = value;
    events.push("set:missing=" + value);
  },
});
show("accessor ??= assign", () => accessor.missing ??= "filled");
console.log("accessor ??= assign setter:", assignedMissing);

const locked: any = {};
Object.defineProperty(locked, "truthy", {
  configurable: true,
  value: "locked",
  writable: false,
});
Object.defineProperty(locked, "zero", {
  configurable: true,
  value: 0,
  writable: false,
});
Object.defineProperty(locked, "nil", {
  configurable: true,
  value: null,
  writable: false,
});

show("nonwritable ||= skip", () => locked.truthy ||= "new");
show("nonwritable &&= skip", () => locked.zero &&= 9);
show("nonwritable ??= throw", () => locked.nil ??= "new");

let keyCount = 0;
function key() {
  keyCount += 1;
  return "truthy";
}
show("computed ||= skip", () => locked[key()] ||= "new");
console.log("computed key count:", keyCount);
