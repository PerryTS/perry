import { Buffer } from "node:buffer";

for (const enc of ["utf8", "utf16le", "latin1", "base64", "base64url", "hex"] as BufferEncoding[]) {
  try { console.log(enc + ":", Buffer.byteLength("hé==", enc)); } catch (err: any) { console.log(enc + ":", err?.name); }
}
console.log("lone high surrogate:", Buffer.byteLength("\ud800", "utf8"));
console.log("invalid encoding accepted:", Buffer.byteLength("abc", "madeup" as any));
