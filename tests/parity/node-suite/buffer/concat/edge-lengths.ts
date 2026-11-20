import { Buffer } from "node:buffer";

const a = Buffer.from("ab");
const b = Buffer.from("cd");
console.log("empty:", Buffer.concat([]).length);
console.log("short:", Buffer.concat([a, b], 3).toString());
console.log("long hex:", Buffer.concat([a, b], 6).toString("hex"));
console.log("uint8:", Buffer.concat([a, new Uint8Array([101, 102]) as any]).toString());
try { Buffer.concat(["x" as any]); console.log("bad no throw"); } catch (err: any) { console.log("bad:", err?.name, err?.code || "no-code"); }
