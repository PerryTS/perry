import { StringDecoder } from "node:string_decoder";

const d = new StringDecoder("utf8");
const u8 = new Uint8Array([104, 105]);
console.log("u8:", d.write(u8));
try { console.log("dataview:", d.write(new DataView(u8.buffer) as any)); } catch (err: any) { console.log("dataview:", err?.name, err?.code || "no-code"); }
try { console.log("string:", d.write("x" as any)); } catch (err: any) { console.log("string:", err?.name, err?.code || "no-code"); }
