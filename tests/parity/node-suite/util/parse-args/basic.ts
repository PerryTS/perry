import { parseArgs } from "node:util";

const result = parseArgs({ args: ["--flag", "--name", "perry", "pos"], options: { flag: { type: "boolean" }, name: { type: "string" } }, allowPositionals: true });
console.log("values:", JSON.stringify(result.values));
console.log("positionals:", result.positionals.join(","));
