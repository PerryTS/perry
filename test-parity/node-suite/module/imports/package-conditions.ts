import imported, { mode as importedMode } from "parity-conditions";
import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
const slash = (value: string) => value.replaceAll("\\", "/");
const required = req("parity-conditions");
console.log("import:", importedMode, imported.mode, imported.marker);
console.log("require:", required.mode, required.marker);
console.log("distinct:", imported !== required);
console.log(
  "resolved branch:",
  slash(req.resolve("parity-conditions")).endsWith(
    "/parity-conditions/require.cjs",
  ),
);
