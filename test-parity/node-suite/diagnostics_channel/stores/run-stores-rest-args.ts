import { channel } from "node:diagnostics_channel";
import { AsyncLocalStorage } from "node:async_hooks";

function describe(value: unknown) {
  return value === undefined ? "undefined" : JSON.stringify(value);
}

const inactive = channel("dc-runstores-rest-inactive");
const thisArg = { tag: "thisArg" };
const inactiveResult = inactive.runStores(
  { value: "inactive" },
  function (this: any, ...args: string[]) {
    return `${this.tag}:${args.join(",")}`;
  },
  thisArg,
  "a",
  "b",
  "c",
);
console.log("inactive result:", inactiveResult);

const active = channel("dc-runstores-rest-active");
const store = new AsyncLocalStorage();
active.bindStore(store);
const activeResult = active.runStores(
  { value: "active" },
  function (this: any, ...args: string[]) {
    console.log("active args:", args.join(","));
    console.log("active this:", this.tag);
    console.log("active store:", describe(store.getStore()));
    return "active-ret";
  },
  thisArg,
  "x",
  "y",
  "z",
);
console.log("active result:", activeResult);
