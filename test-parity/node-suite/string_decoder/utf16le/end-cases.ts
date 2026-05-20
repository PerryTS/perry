import { StringDecoder } from "node:string_decoder";

for (const [first, next] of [["3D", "6100"], ["3DD8", ""], ["3DD8", "6100"], ["3DD84D", "DC"]]) {
  const dec = new StringDecoder("utf16le");
  const output = dec.write(Buffer.from(first, "hex")) + dec.end() + dec.write(Buffer.from(next, "hex")) + dec.end();
  console.log(first + "/" + next + ":", JSON.stringify(output));
}
