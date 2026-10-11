import { StringDecoder } from "node:string_decoder";
const decoder = new StringDecoder("utf8");
console.log("decoder", decoder.write(Buffer.from("live")));
const write = StringDecoder.prototype.write;
for (const n of [1, 2, 3, 4, 0xFFFFF, 0x100000, 0x180000, 0x1FFFFF, 0x200000, Number.MIN_VALUE]) {
  let result = "";
  try { result = "accepted " + write.call(n as any, Buffer.from("x")); }
  catch (error) { result = (error as any).code; }
  console.log("number-receiver", n, result);
}

