import { channel, Channel, hasSubscribers } from "node:diagnostics_channel";

const a = channel("dc-identity");
const b = channel("dc-identity");
const c = channel("dc-other");
console.log("same name identity:", a === b);
console.log("different name identity:", a === c);
console.log("name:", a.name);
console.log("instanceof Channel:", a instanceof Channel);
console.log("initial hasSubscribers fn:", hasSubscribers("dc-identity"));
console.log("initial hasSubscribers prop:", a.hasSubscribers);
