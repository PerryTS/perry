import { parseArgs } from "node:util";

try {
  const result = parseArgs({ args: ["--no-color", "--count=-1"], options: { color: { type: "boolean" }, count: { type: "string" } } });
  console.log("values:", JSON.stringify(result.values));
} catch (err: any) { console.log("no-color:", err?.name, err?.code || "no-code"); }
try { parseArgs({ args: ["--flag=value"], options: { flag: { type: "boolean" } } }); console.log("bool value no throw"); } catch (err: any) { console.log("bool value:", err?.name, err?.code || "no-code"); }
