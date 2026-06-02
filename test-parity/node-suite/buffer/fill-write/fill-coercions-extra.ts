import { Buffer } from "node:buffer";

function show(label: string, fn: () => unknown): void {
  const b = Buffer.alloc(5, ".");
  try { console.log(label + ":", fn.call(null, b), b.toString("hex")); } catch (err: any) { console.log(label + ":", err?.name, err?.code || "no-code"); }
}
show("fill number", (b: Buffer) => b.fill(257));
show("fill bool", (b: Buffer) => b.fill(true as any));
show("fill empty string", (b: Buffer) => b.fill(""));
show("fill uint8", (b: Buffer) => b.fill(new Uint8Array([65, 66]) as any));
