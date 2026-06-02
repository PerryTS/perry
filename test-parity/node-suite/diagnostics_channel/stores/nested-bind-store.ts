import { channel } from "node:diagnostics_channel";

const ch: any = channel("dc-nested-bind-store");
if (typeof ch.bindStore !== "function") {
  console.log("bindStore type:", typeof ch.bindStore);
} else {
  const store = { value: 1 };
  ch.bindStore(store);
  console.log("has after bind:", ch.hasSubscribers);
  ch.unbindStore(store);
  console.log("has after unbind:", ch.hasSubscribers);
}
