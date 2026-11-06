import { StringDecoder } from "node:string_decoder";

for (const enc of [undefined, "", "UTF-8", "Utf16LE", "BASE64"] as any[]) {
  try { const d = new StringDecoder(enc); console.log("enc:", String(enc), "=>", d.encoding); } catch (err: any) { console.log("enc:", String(enc), err?.name, err?.code || "no-code"); }
}
