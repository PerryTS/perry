// #3082: Channel#runStores(context, fn[, thisArg[, ...args]]) must forward the
// full trailing argument list to `fn` and bind `thisArg` as the receiver, both
// with and without a bound store, returning the callback's value.
import diagnostics_channel from "node:diagnostics_channel";
import { AsyncLocalStorage } from "node:async_hooks";

// No bound store: thisArg bound, three extra args forwarded, value returned.
const plain = diagnostics_channel.channel("dc:gap:runstores:plain");
const plainRet = plain.runStores(
  { phase: "plain" },
  function (...args: unknown[]): string {
    console.log("plain args:", JSON.stringify(args));
    console.log("plain this.tag:", (this as { tag?: string }).tag);
    return "plain-ret";
  },
  { tag: "plain-ctx" },
  "a",
  "b",
  "c",
);
console.log("plain ret:", plainRet);

// Bound store: full arg list forwarded through the store-run chain, store
// visible inside the callback, thisArg bound, value returned.
const bound = diagnostics_channel.channel("dc:gap:runstores:bound");
const als = new AsyncLocalStorage<{ value: number }>();
bound.bindStore(als);
const boundRet = bound.runStores(
  { value: 42 },
  function (...args: unknown[]): string {
    console.log("bound args:", JSON.stringify(args));
    console.log("bound this.tag:", (this as { tag?: string }).tag);
    console.log("bound store:", JSON.stringify(als.getStore()));
    return "bound-ret";
  },
  { tag: "bound-ctx" },
  1,
  2,
  3,
);
console.log("bound ret:", boundRet);

// Zero extra args: callback receives no positional args.
const none = diagnostics_channel.channel("dc:gap:runstores:none");
const noneRet = none.runStores({}, function (a: unknown, b: unknown): string {
  return String(a) + "/" + String(b);
});
console.log("none ret:", noneRet);
