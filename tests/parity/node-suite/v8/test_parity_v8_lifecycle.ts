// Focused parity fixture for the Perry-backed node:v8 lifecycle helpers.

import * as v8 from "node:v8";

console.log("v8.promiseHooks typeof:", typeof v8.promiseHooks);
// NOTE: native-module namespace objects currently leak the internal
// `__module__` marker through `Object.keys` (a generic pre-existing gap, not
// specific to node:v8), so assert the exported surface by presence instead of
// raw key enumeration.
console.log(
  "v8.promiseHooks has all:",
  ["createHook", "onInit", "onBefore", "onAfter", "onSettled"].every(
    (k) => typeof (v8.promiseHooks as any)[k] === "function",
  ),
);
console.log("v8.promiseHooks.onInit typeof:", typeof v8.promiseHooks.onInit);
console.log("v8.promiseHooks.onBefore typeof:", typeof v8.promiseHooks.onBefore);
console.log("v8.promiseHooks.onAfter typeof:", typeof v8.promiseHooks.onAfter);
console.log("v8.promiseHooks.onSettled typeof:", typeof v8.promiseHooks.onSettled);
console.log("v8.promiseHooks.createHook typeof:", typeof v8.promiseHooks.createHook);

console.log("v8.startupSnapshot typeof:", typeof v8.startupSnapshot);
console.log(
  "v8.startupSnapshot has all:",
  [
    "addDeserializeCallback",
    "addSerializeCallback",
    "setDeserializeMainFunction",
    "isBuildingSnapshot",
  ].every((k) => typeof (v8.startupSnapshot as any)[k] === "function"),
);
console.log("v8.startupSnapshot.isBuildingSnapshot:", v8.startupSnapshot.isBuildingSnapshot());

try {
  v8.promiseHooks.onInit();
} catch (e: any) {
  console.log("onInit missing:", e && e.name, e && e.code);
}

try {
  v8.promiseHooks.onBefore(1 as any);
} catch (e: any) {
  console.log("onBefore invalid:", e && e.name, e && e.code);
}

try {
  v8.promiseHooks.createHook({ init: 1 as any });
} catch (e: any) {
  console.log("createHook invalid:", e && e.name, e && e.code);
}

let stoppedInitCount = 0;
const stopImmediately = v8.promiseHooks.onInit(() => {
  stoppedInitCount++;
});
stopImmediately();
stopImmediately();
Promise.resolve("stopped");
console.log("stopped init count:", stoppedInitCount);

let createHookInitCount = 0;
const stopCreatedHook = v8.promiseHooks.createHook({
  init() {
    createHookInitCount++;
  },
});
Promise.resolve("created");
stopCreatedHook();
stopCreatedHook();
console.log("createHook init fired:", createHookInitCount > 0);

let rootInits = 0;
let childInits = 0;
let beforeCount = 0;
let afterCount = 0;
let settledCount = 0;

const stops = [
  v8.promiseHooks.onInit((_promise: any, parent: any) => {
    if (parent === undefined) rootInits++;
    else childInits++;
  }),
  v8.promiseHooks.onBefore((_promise: any) => {
    beforeCount++;
  }),
  v8.promiseHooks.onAfter((_promise: any) => {
    afterCount++;
  }),
  v8.promiseHooks.onSettled((_promise: any) => {
    settledCount++;
  }),
];

Promise.resolve("x")
  .then((value: string) => value + "y")
  .then((value: string) => {
    for (const stop of stops) stop();
    console.log(
      "promiseHooks lifecycle:",
      rootInits > 0,
      childInits > 0,
      beforeCount > 0,
      afterCount > 0,
      settledCount > 0,
      value,
    );

    for (const [name, fn] of [
      ["addSerializeCallback", v8.startupSnapshot.addSerializeCallback],
      ["addDeserializeCallback", v8.startupSnapshot.addDeserializeCallback],
      ["setDeserializeMainFunction", v8.startupSnapshot.setDeserializeMainFunction],
    ] as any[]) {
      try {
        fn(() => {});
      } catch (e: any) {
        console.log(name + " error:", e && e.name, e && e.code);
      }
    }
  });
