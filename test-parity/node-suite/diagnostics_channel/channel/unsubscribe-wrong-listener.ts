import { channel } from "node:diagnostics_channel";
const ch = channel("dc-unsubscribe-wrong");
function a() {}
function b() {}
ch.subscribe(a);
console.log("wrong:", ch.unsubscribe(b));
console.log("has:", ch.hasSubscribers);
console.log("right:", ch.unsubscribe(a));
