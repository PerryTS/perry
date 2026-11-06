import { parseArgs } from "node:util";

const result = parseArgs({ args: ["-a", "1", "--", "tail"], options: { a: { type: "string", short: "a" } }, tokens: true, allowPositionals: true });
console.log("values:", JSON.stringify(result.values));
console.log("tokens:", result.tokens?.map(t => t.kind + ":" + (t.name || t.value || "")).join(","));
try { parseArgs({ args: ["--unknown"], options: {}, strict: true }); console.log("strict no throw"); } catch (err: any) { console.log("strict:", err?.name, err?.code || "no-code"); }
