import { Readable } from "node:stream";
// readable.readableEncoding reflects the encoding configured via
// constructor option or setEncoding(); null when none is set.
const r1 = new Readable({ read() {} });
console.log("default encoding:", r1.readableEncoding);
r1.setEncoding("utf8");
console.log("after setEncoding utf8:", r1.readableEncoding);

const r2 = new Readable({ encoding: "hex", read() {} });
console.log("constructor encoding:", r2.readableEncoding);
