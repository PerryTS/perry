// process.loadEnvFile(path?) loads a .env file (Node 20.12+).
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

console.log("is function:", typeof process.loadEnvFile === "function");

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "perry-loadenv-"));
const cwd = process.cwd();
const file = path.join(tmp, "vars.env");
const defaultFile = path.join(tmp, ".env");
const content = [
  "A=1",
  "B = two # comment",
  'C="three # not comment"',
  "D=unquoted value # comment",
  "export E=5",
  'MULTI="line1',
  'line2"',
  "EXISTING=file",
].join("\n");

fs.writeFileSync(file, content);
fs.writeFileSync(defaultFile, "DEFAULT=ok\n");

const keys = ["A", "B", "C", "D", "E", "MULTI", "EXISTING", "DEFAULT"];

function clearEnv() {
  for (const key of keys) {
    delete process.env[key];
  }
}

function snapshot() {
  return keys
    .map((key) => key + "=" + JSON.stringify(process.env[key] ?? "unset"))
    .join(";");
}

function reportLoad(label, value, passArg = true) {
  clearEnv();
  process.env.EXISTING = "before";
  try {
    const result = passArg ? process.loadEnvFile(value) : process.loadEnvFile();
    console.log(label, "OK", String(result), snapshot());
  } catch (err) {
    const e = err;
    console.log(
      label,
      "THROW",
      e.name,
      e.code || "nocode",
      String(e.message).split("\n")[0],
    );
  }
}

reportLoad("string", file);
reportLoad("buffer", Buffer.from(file));
reportLoad("url", pathToFileURL(file));

process.chdir(tmp);
reportLoad("omitted", undefined, false);
reportLoad("undefined", undefined);
reportLoad("null", null);
process.chdir(cwd);

reportLoad("file-url-encoded-slash", new URL("file:///tmp/a%2Fb.env"));
reportLoad("number", 123);
reportLoad("boolean", true);
reportLoad("object", {});
reportLoad("array", []);
reportLoad("symbol", Symbol("x"));
reportLoad("http-url", new URL("https://example.test/x"));
