import { channel } from "node:diagnostics_channel";
import { AsyncLocalStorage } from "node:async_hooks";

const ch = channel("dc-with-store-scope");
const store1 = new AsyncLocalStorage();
const store2 = new AsyncLocalStorage();

ch.bindStore(store1);
ch.bindStore(store2, (data: any) => ({ wrapped: data.value }));

console.log("withStoreScope typeof:", typeof (ch as any).withStoreScope);
console.log("before:", store1.getStore(), store2.getStore());

let inside: string;
let nested: string;
let afterNested: string;
let disposeReturn: any;

{
  using scope = (ch as any).withStoreScope({ value: "outer" });
  inside = [store1.getStore()?.value, store2.getStore()?.wrapped].join(",");
  {
    using nestedScope = (ch as any).withStoreScope({ value: "inner" });
    nested = [store1.getStore()?.value, store2.getStore()?.wrapped].join(",");
  }
  afterNested = [store1.getStore()?.value, store2.getStore()?.wrapped].join(",");
  disposeReturn = (scope as any)[Symbol.dispose]();
}

console.log("inside:", inside);
console.log("nested:", nested);
console.log("after nested:", afterNested);
console.log("manual dispose return:", disposeReturn);
console.log("after:", store1.getStore(), store2.getStore());
