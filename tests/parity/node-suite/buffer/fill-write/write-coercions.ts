import { Buffer } from "node:buffer";

const b = Buffer.alloc(6, ".");
function show(label: string, fn: () => unknown): void {
  try { console.log(label + ":", fn(), b.toString()); } catch (err: any) { console.log(label + ":", err?.name, err?.code || "no-code"); }
}
show("write bool", () => b.write("abc", true as any));
show("write null len", () => b.write("XYZ", 2, null as any));
show("write bad enc", () => b.write("x", 0, 1, "bad" as any));
show("fill bad range", () => b.fill("z", 5, 2));
