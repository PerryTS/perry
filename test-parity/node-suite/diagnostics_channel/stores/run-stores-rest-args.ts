import { AsyncLocalStorage } from "node:async_hooks";
import { channel } from "node:diagnostics_channel";

const bound = channel("dc-runstores-rest-bound");
const store = new AsyncLocalStorage();
bound.bindStore(store);

const boundRet = bound.runStores({ value: 1 }, function (this: any, ...args: any[]) {
  console.log("bound args:", JSON.stringify(args), this.tag, JSON.stringify(store.getStore()));
  return "bound-ret";
}, { tag: "bound-this" }, "a", "b", "c");
console.log("bound ret:", boundRet);

const plain = channel("dc-runstores-rest-plain");
const plainRet = plain.runStores({ value: 2 }, function (this: any, ...args: any[]) {
  console.log("plain args:", JSON.stringify(args), this.tag);
  return "plain-ret";
}, { tag: "plain-this" }, 1, true, null);
console.log("plain ret:", plainRet);
