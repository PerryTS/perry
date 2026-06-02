import { StringDecoder } from "node:string_decoder";

for (const bytes of [[0x80], [0xc0, 0xaf], [0xe2, 0x28, 0xa1], [0xf0, 0x28, 0x8c, 0xbc]]) {
  const d = new StringDecoder("utf8");
  console.log("invalid:", Buffer.from(bytes).toString("hex"), JSON.stringify(d.write(Buffer.from(bytes)) + d.end()));
}
