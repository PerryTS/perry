import { Buffer } from "node:buffer";

const a = Buffer.from("abcdef");
const b = Buffer.from("abcxyz");
function show(label: string, fn: () => unknown): void {
  try { console.log(label + ":", fn()); } catch (err: any) { console.log(label + ":", err?.name, err?.code || "no-code"); }
}
show("range equal", () => a.compare(b, 0, 3, 0, 3));
show("range diff", () => a.compare(b, 3, 6, 3, 6));
show("targetStart negative", () => a.compare(b, -1));
show("sourceEnd large", () => a.compare(b, 0, 3, 0, 99));
show("uint8 target", () => a.compare(new Uint8Array([97, 98, 99]) as any));
