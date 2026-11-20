import { channel, hasSubscribers } from "node:diagnostics_channel";
const ch = channel("");
console.log("name:", JSON.stringify(ch.name));
console.log("identity:", ch === channel(""));
console.log("has:", hasSubscribers(""));
