import path from "node:path";

for (const args of [["a", 1], ["a", null], ["a", {}], ["a", []]] as any[]) {
  try { console.log("join:", path.join(...args)); } catch (err: any) { console.log("join err:", err?.name, err?.code || "no-code"); }
}
