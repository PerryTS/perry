import { channel } from "node:diagnostics_channel";

for (const name of ["", "0", Symbol.for("dc-name")] as any[]) {
  try { const ch = channel(name); console.log("name:", String(name), String(ch.name), ch === channel(name)); } catch (err: any) { console.log("name err:", String(name), err?.name, err?.code || "no-code"); }
}
try { channel({} as any); console.log("object name no throw"); } catch (err: any) { console.log("object name:", err?.name, err?.code || "no-code"); }
