import { promiseHooks } from "node:v8";

let seenPromise: Promise<any> | undefined;
let seenParent: Promise<any> | undefined;
const stop = promiseHooks.onInit((promise, parent) => {
  seenPromise = promise;
  seenParent = parent;
});

const parent = Promise.resolve(1);
console.log("root:", seenPromise === parent, seenParent === undefined);
const child = parent.then((value) => value + 1);
console.log("child:", seenPromise === child, seenParent === parent);
stop();
seenPromise = undefined;
Promise.resolve(2);
console.log("stopped:", seenPromise === undefined);
await child;
