import { StringDecoder } from "node:string_decoder";

const bytes = Buffer.from("hello world").toString("base64");
for (let split = 0; split <= bytes.length; split += 3) {
  const d = new StringDecoder("base64");
  const out = d.write(Buffer.from(bytes.slice(0, split))) + d.write(Buffer.from(bytes.slice(split))) + d.end();
  console.log("split", split + ":", out);
}
