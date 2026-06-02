import { Buffer } from "node:buffer";

const fill = new Uint8Array([65, 66]);
console.log("typed fill:", Buffer.alloc(5, fill).toString());
console.log("zero len fill:", Buffer.alloc(0, fill).length);
try { console.log("negative size:", Buffer.alloc(-1 as any).length); } catch (err: any) { console.log("negative size:", err?.name, err?.code || "no-code"); }
