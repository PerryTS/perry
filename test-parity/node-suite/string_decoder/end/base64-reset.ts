import { StringDecoder } from "node:string_decoder";

for (const enc of ["base64", "base64url"] as const) {
  const dec = new StringDecoder(enc);
  let out = "";
  out += dec.write(Buffer.from([0x61]));
  out += dec.end();
  out += dec.write(Buffer.from([0x61]));
  out += dec.end();
  console.log(enc + ":", out);
}
