import { StringDecoder } from "node:string_decoder";

const bytes = Buffer.from("a€𐍈b");
for (let split = 0; split <= bytes.length; split++) {
  const d = new StringDecoder("utf8");
  const out = d.write(bytes.subarray(0, split)) + d.write(bytes.subarray(split)) + d.end();
  console.log("split", split + ":", out);
}
