import { StringDecoder } from "node:string_decoder";

for (const hex of ["C9B5A941", "E241", "CCCCB8", "F0FB00", "E2FBCC01", "EDA0B5EDB08D"]) {
  const dec = new StringDecoder("utf8");
  console.log(hex + ":", JSON.stringify(dec.write(Buffer.from(hex, "hex")) + dec.end()));
}
