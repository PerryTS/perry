// Bun 1.3.14 oracle, also run by crates/perry/tests/issue_10104_runtime_data_imports.rs.
import { writeFileSync, readFileSync, existsSync } from "node:fs";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { pathToFileURL } from "node:url";

const directory = process.cwd();
const legacy = path.join(directory, "config space # %.legacy");
writeFileSync(legacy, 'provider = "anthropic"\nmodel = "claude-sonnet-4-5"\ntheme = "dark"\n');

const load = (specifier: string, type: string) => import(specifier, { with: { type } });
const table = await load(pathToFileURL(legacy).href, "toml");
console.log("toml", table.default.provider, table.default.model, table.default.theme);

const jsonPath = path.join(directory, "data.json");
writeFileSync(jsonPath, '{"answer":42,"nested":{"enabled":true}}');
const type = "json";
const captured = (specifier: string) => import(specifier, { with: { type } });
console.log("json", (await captured(jsonPath)).default.answer);
console.log("json url", (await load(pathToFileURL(jsonPath).href, "json")).default.nested.enabled);

const textPath = path.join(directory, "content space # %.data");
writeFileSync(textPath, "hello π\n");
console.log("text", JSON.stringify((await load(pathToFileURL(textPath).href, "text")).default));
const assetPath = path.join(directory, "asset space # %.data");
writeFileSync(assetPath, "asset");
console.log("file", path.normalize((await load(pathToFileURL(assetPath).href, "file")).default) === assetPath);

let order = "";
function specifier() { order += "s"; return jsonPath; }
function options() { order += "o"; return { with: { type: "json" } }; }
console.log("options", (await import(specifier(), options())).default.answer, order);

const badToml = path.join(directory, "bad.toml");
const badJson = path.join(directory, "bad.json");
writeFileSync(badToml, "broken = [");
writeFileSync(badJson, "{broken");
await load(pathToFileURL(badToml).href, "toml").catch((error: any) => {
  // Bun 1.3.14 wraps TOML loader diagnostics in BuildMessage; Perry's
  // requested SyntaxError contract is asserted separately in the Rust suite.
  console.log("bad toml", !!error);
});
await load(badJson, "json").catch((error: any) => {
  console.log("bad json", error.name, error instanceof SyntaxError);
});

// OpenCode's legacy migration: destructure the imported default, combine the
// provider/model, write config.json, and remove the old file. The swallowing
// catch is intentional: the pre-fix runtime silently skipped this migration.
let result: any = {};
await import(pathToFileURL(legacy).href, { with: { type: "toml" } })
  .then(async (mod) => {
    const { provider, model, ...rest } = mod.default;
    if (provider && model) result.model = `${provider}/${model}`;
    result["$schema"] = "https://opencode.ai/config.json";
    result = Object.assign(result, rest);
    await fs.writeFile(path.join(directory, "config.json"), JSON.stringify(result, null, 2));
    await fs.unlink(legacy);
  })
  .catch(() => {});
console.log("migrated", !existsSync(legacy));
console.log(readFileSync(path.join(directory, "config.json"), "utf8"));
