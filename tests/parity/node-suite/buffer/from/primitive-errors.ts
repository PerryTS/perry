import { Buffer } from "node:buffer";

for (const value of [null, undefined, 123, true, Symbol("s"), 10n] as any[]) {
  try { console.log("from", String(value) + ":", Buffer.from(value).length); } catch (err: any) { console.log("from", String(value) + ":", err?.name, err?.code || "no-code"); }
}
