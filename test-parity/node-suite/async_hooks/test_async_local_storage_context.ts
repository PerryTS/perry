import { AsyncLocalStorage } from "node:async_hooks";
import { setImmediate } from "node:timers";

const als = new AsyncLocalStorage();
const other = new AsyncLocalStorage();

async function viaAwait() {
  await Promise.resolve();
  console.log("await store:", als.getStore().trace);
}

function viaThen() {
  return Promise.resolve().then(() => {
    console.log("then store:", als.getStore().trace);
  });
}

function viaCatch() {
  return Promise.reject(new Error("boom")).catch(() => {
    console.log("catch store:", als.getStore().trace);
  });
}

function viaNextTick() {
  return new Promise((resolve) => {
    process.nextTick(() => {
      console.log("nextTick store:", als.getStore().trace);
      resolve(undefined);
    });
  });
}

function viaTimer() {
  return new Promise((resolve) => {
    setTimeout(() => {
      console.log("timer store:", als.getStore().trace);
      resolve(undefined);
    }, 0);
  });
}

function viaImmediate() {
  return new Promise((resolve) => {
    setImmediate(() => {
      console.log("immediate store:", als.getStore().trace);
      resolve(undefined);
    });
  });
}

function viaInterval() {
  return new Promise((resolve) => {
    let id: any;
    id = setInterval(() => {
      clearInterval(id);
      console.log("interval store:", als.getStore().trace);
      resolve(undefined);
    }, 0);
  });
}

async function nested() {
  await als.run({ trace: "outer" }, async () => {
    console.log("outer pre:", als.getStore().trace);
    await als.run({ trace: "inner" }, async () => {
      await Promise.resolve();
      console.log("inner:", als.getStore().trace);
    });
    await Promise.resolve();
    console.log("outer post:", als.getStore().trace);
  });
}

async function enterWithContext() {
  als.enterWith({ trace: "entered" });
  await Promise.resolve();
  console.log("enterWith store:", als.getStore().trace);
  als.disable();
}

async function exitContextIsolation() {
  await als.run({ trace: "exit-outer" }, async () => {
    await other.run({ trace: "other-before" }, async () => {
      await als.exit(async () => {
        console.log("exit clears primary:", als.getStore());
        other.enterWith({ trace: "other-mutated" });
        await Promise.resolve();
        console.log("exit keeps other:", other.getStore().trace);
      });
      console.log("exit restores primary:", als.getStore().trace);
      console.log("exit preserves other mutation:", other.getStore().trace);
    });
  });
}

async function twoInstances() {
  await als.run({ trace: "primary" }, async () => {
    await other.run({ trace: "secondary" }, async () => {
      await Promise.resolve();
      console.log("primary store:", als.getStore().trace);
      console.log("secondary store:", other.getStore().trace);
    });
  });
}

(async () => {
  await als.run({ trace: "await" }, viaAwait);
  await als.run({ trace: "then" }, viaThen);
  await als.run({ trace: "catch" }, viaCatch);
  await als.run({ trace: "nextTick" }, viaNextTick);
  await als.run({ trace: "timer" }, viaTimer);
  await als.run({ trace: "immediate" }, viaImmediate);
  await als.run({ trace: "interval" }, viaInterval);
  await nested();
  await enterWithContext();
  await exitContextIsolation();
  await twoInstances();
})();
