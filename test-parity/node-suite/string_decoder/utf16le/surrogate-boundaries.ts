import { StringDecoder } from "node:string_decoder";

const bytes = Buffer.from("a😀b", "utf16le");
for (let split = 0; split <= bytes.length; split++) {
  const d = new StringDecoder("utf16le");
  const out = d.write(bytes.subarray(0, split)) + d.write(bytes.subarray(split)) + d.end();
  console.log("split", split + ":", out);
}
