import path from "node:path";

for (const value of [null, undefined, 1, {}, []] as any[]) {
  try { console.log("resolve:", path.resolve("/tmp", value)); } catch (err: any) { console.log("resolve err:", err?.name, err?.code || "no-code"); }
}
