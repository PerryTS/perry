import { StringDecoder } from "node:string_decoder";

for (const enc of ["utf8", "utf-8", "ucs2", "ucs-2", "utf16le", "latin1", "binary", "base64", "hex"] as any[]) {
  try { const d = new StringDecoder(enc); console.log(enc + ":", d.encoding, d.write(Buffer.from("6869", "hex"))); } catch (err: any) { console.log(enc + ":", err?.name, err?.code || "no-code"); }
}
