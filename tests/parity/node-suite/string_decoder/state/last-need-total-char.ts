import { StringDecoder } from "node:string_decoder";

const dec = new StringDecoder("utf8");
console.log("initial:", (dec as any).lastNeed, (dec as any).lastTotal, Buffer.from((dec as any).lastChar).toString("hex"));
console.log("write:", JSON.stringify(dec.write(Buffer.from("E1", "hex"))));
console.log("partial:", (dec as any).lastNeed, (dec as any).lastTotal, Buffer.from((dec as any).lastChar).toString("hex"));
console.log("end:", JSON.stringify(dec.end()));
console.log("after:", (dec as any).lastNeed, (dec as any).lastTotal, Buffer.from((dec as any).lastChar).toString("hex"));
