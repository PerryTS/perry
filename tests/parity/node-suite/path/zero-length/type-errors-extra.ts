import path from "node:path";

for (const value of [null, undefined, 1, {}, []] as any[]) {
  try { console.log("basename", String(value) + ":", path.basename(value)); } catch (err: any) { console.log("basename", String(value) + ":", err?.name, err?.code || "no-code"); }
}
