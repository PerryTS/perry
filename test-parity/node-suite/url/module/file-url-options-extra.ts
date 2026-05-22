import { fileURLToPath, pathToFileURL } from "node:url";

const u = pathToFileURL("/tmp/a#b?c");
console.log("escaped path:", u.href);
console.log("roundtrip:", fileURLToPath(u));
try { fileURLToPath("https://example.com/"); console.log("https no throw"); } catch (err: any) { console.log("https:", err?.name, err?.code || "no-code"); }
