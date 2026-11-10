import { deprecate } from "node:util";

const events: string[] = [];
process.on("warning", (warning: any) => {
  events.push(`${warning.name}:${warning.code ?? ""}:${warning.message}`);
  console.log("event:", events.join("|"));
});

const fn = deprecate((x: number) => x + 1, "deprecated fn", "DEP_PERRY_TEST");
console.log("shape:", typeof fn, fn.name, fn.length);
console.log("call1:", fn(1));
console.log("call2:", fn(2));
setImmediate(() => console.log("warnings:", JSON.stringify(events)));
try { deprecate(() => 1, "msg", "bad code with spaces"); console.log("bad code no throw"); } catch (err: any) { console.log("bad code:", err?.name, err?.code || "no-code"); }
