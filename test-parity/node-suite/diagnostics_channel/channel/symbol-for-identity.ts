import { channel } from "node:diagnostics_channel";
const a = Symbol.for("dc-symbol-for");
const b = Symbol.for("dc-symbol-for");
console.log("same symbol channel:", channel(a) === channel(b));
console.log("name same:", channel(a).name === a);
