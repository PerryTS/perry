import { channel } from "node:diagnostics_channel";
import { AsyncLocalStorage } from "node:async_hooks";

const ch = channel("dc-transform-error");
const store = new AsyncLocalStorage();
const error = new Error("transform boom");
let returned = false;
process.once("uncaughtException", (err: Error) => {
  console.log("uncaught same error:", err === error);
  console.log("returned before uncaught:", returned);
});
ch.bindStore(store, () => {
  throw error;
});
const result = ch.runStores({ value: 1 }, () => {
  console.log("callback ran store:", store.getStore());
  return "ok";
});
returned = true;
console.log("runStores result:", result);
