import { StringDecoder } from "node:string_decoder";

for (const enc of ["base64", "base64url", "hex", "utf8", "utf16le", "ucs2"] as const) {
  const input = Buffer.from("asdf");
  const dec = new StringDecoder(enc);
  let out = "";
  for (let i = 0; i < input.length; i++) {
    out += dec.write(input.subarray(i, i + 1));
  }
  out += dec.end();
  console.log(enc + ":", JSON.stringify(out));
}
