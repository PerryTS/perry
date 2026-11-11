import tty from "node:tty";

for (const fd of [-1, 0, 1, 2, 999999] as any[]) {
  try { console.log("isatty", fd + ":", tty.isatty(fd)); } catch (err: any) { console.log("isatty", fd + ":", err?.name, err?.code || "no-code"); }
}
try { console.log("isatty string:", tty.isatty("1" as any)); } catch (err: any) { console.log("isatty string:", err?.name, err?.code || "no-code"); }
