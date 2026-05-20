import { StringDecoder } from "node:string_decoder";

for (const hex of ["E2", "E282", "F09F", "F09F98"]) {
  const dec = new StringDecoder("utf8");
  console.log(hex, "write:", JSON.stringify(dec.write(Buffer.from(hex, "hex"))));
  console.log(hex, "end:", JSON.stringify(dec.end()));
}
