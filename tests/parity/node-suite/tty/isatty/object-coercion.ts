import tty from "node:tty";

const obj = { valueOf() { return 1; } };
try { console.log("object:", tty.isatty(obj as any)); } catch (err: any) { console.log("object:", err?.name, err?.code || "no-code"); }
try { console.log("bigint:", tty.isatty(1n as any)); } catch (err: any) { console.log("bigint:", err?.name, err?.code || "no-code"); }
