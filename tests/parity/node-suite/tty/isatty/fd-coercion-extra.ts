import tty from "node:tty";

for (const fd of [NaN, Infinity, 1.5, true, null] as any[]) {
  try { console.log("fd", String(fd) + ":", tty.isatty(fd)); } catch (err: any) { console.log("fd", String(fd) + ":", err?.name, err?.code || "no-code"); }
}
