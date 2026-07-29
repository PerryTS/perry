import { promiseHooks } from "node:v8";

const events: string[] = [];
let root: Promise<any>;
let child: Promise<any>;
const stop = promiseHooks.createHook({
  init(promise, parent) {
    if (parent === undefined) {
      root = promise;
      events.push("init:root");
    } else if (parent === root) {
      events.push("init:child:parent");
    }
  },
  before(promise) {
    if (promise === child) events.push("before:child");
  },
  after(promise) {
    if (promise === child) events.push("after:child");
  },
  settled(promise) {
    if (promise === root) events.push("settled:root");
    if (promise === child) events.push("settled:child");
  },
});

root = Promise.resolve("value");
child = root.then(() => {
  events.push("callback");
});
child.then(() => {
  stop();
  console.log("events:", events.join(","));
});
