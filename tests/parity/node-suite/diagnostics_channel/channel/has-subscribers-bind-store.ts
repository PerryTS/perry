import { channel } from "node:diagnostics_channel";
import { AsyncLocalStorage } from "node:async_hooks";

const ch = channel("dc-store-has-subscribers");
const store = new AsyncLocalStorage();
console.log("initial channel:", ch.hasSubscribers);
ch.bindStore(store);
console.log("after bind channel:", ch.hasSubscribers);
console.log("after bind module:", channel("dc-store-has-subscribers").hasSubscribers);
console.log("unbind:", ch.unbindStore(store));
console.log("after unbind:", ch.hasSubscribers);
