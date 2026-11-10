import { StringDecoder } from "node:string_decoder";

for (const enc of [undefined, "utf8", "utf-8", "ucs2", "ucs-2", "utf16le", "utf-16le"] as const) {
  const dec = enc === undefined ? new StringDecoder() : new StringDecoder(enc);
  console.log(String(enc), "encoding:", (dec as any).encoding);
}
