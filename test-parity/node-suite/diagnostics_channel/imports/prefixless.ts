import dcDefault, { channel } from "diagnostics_channel";
import * as dc from "diagnostics_channel";

const ch1 = channel("dc-prefixless");
const ch2 = dc.channel("dc-prefixless");
console.log("prefixless namespace type:", typeof dc.channel);
console.log("prefixless default type:", typeof dcDefault.channel);
console.log("prefixless identity:", ch1 === ch2);
console.log("prefixless name:", String(ch1.name));
