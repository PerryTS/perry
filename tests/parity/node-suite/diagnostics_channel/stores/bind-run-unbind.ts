import { channel } from "node:diagnostics_channel";
import { AsyncLocalStorage } from "node:async_hooks";

const ch = channel("dc-bind-store");
const store1 = new AsyncLocalStorage();
const store2 = new AsyncLocalStorage();
const events: string[] = [];
const thisArg = { name: "thisArg" };

ch.bindStore(store1);
ch.bindStore(store2, (data: any) => ({ wrapped: data.value }));
ch.subscribe((data: any) => {
  events.push(`sub:${data.value}:${store1.getStore()?.value}:${store2.getStore()?.wrapped}`);
});

console.log("before stores:", store1.getStore(), store2.getStore());
const result = ch.runStores({ value: "outer" }, function (this: any, a: string, b: string) {
  events.push(`fn:${this === thisArg}:${a}:${b}:${store1.getStore()?.value}:${store2.getStore()?.wrapped}`);
  ch.runStores({ value: "inner" }, () => {
    events.push(`inner:${store1.getStore()?.value}:${store2.getStore()?.wrapped}`);
  });
  events.push(`after-inner:${store1.getStore()?.value}:${store2.getStore()?.wrapped}`);
  return "result";
}, thisArg, "a", "b");

console.log("result:", result);
console.log("events:", events.join("|"));
console.log("after stores:", store1.getStore(), store2.getStore());
console.log("unbind store1:", ch.unbindStore(store1));
console.log("unbind store1 again:", ch.unbindStore(store1));
ch.runStores({ value: "after" }, () => {
  console.log("after unbind store1:", store1.getStore(), store2.getStore()?.wrapped);
});
