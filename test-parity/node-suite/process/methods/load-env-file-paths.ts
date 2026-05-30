import * as fs from "node:fs";
import { pathToFileURL } from "node:url";

const ROOT = "/tmp/perry_node_suite_process_loadenv_paths";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_err) {}
fs.mkdirSync(ROOT, { recursive: true });
const file = ROOT + "/.env";
fs.writeFileSync(file, "PERRY_LOADENV_PATH=ok\n");

const before = process.cwd();
process.chdir(ROOT);

function reset() {
  delete process.env.PERRY_LOADENV_PATH;
}

function run(label: string, fn: () => any) {
  reset();
  try {
    const ret = fn();
    console.log(label, "OK", ret === undefined, process.env.PERRY_LOADENV_PATH || "unset");
  } catch (err: any) {
    console.log(label, "THROW", err.name, err.code);
  }
}

run("omitted", () => process.loadEnvFile());
run("undefined", () => process.loadEnvFile(undefined as any));
run("null", () => process.loadEnvFile(null as any));
run("string", () => process.loadEnvFile(file));
run("buffer", () => process.loadEnvFile(Buffer.from(file) as any));
run("url", () => process.loadEnvFile(pathToFileURL(file) as any));
run("url-http", () => process.loadEnvFile(new URL("https://example.com/.env") as any));
run("url-file-encoded-slash", () => process.loadEnvFile(new URL("file:///tmp/a%2Fb.env") as any));
run("number", () => process.loadEnvFile(123 as any));
run("boolean", () => process.loadEnvFile(true as any));
run("object", () => process.loadEnvFile({} as any));
run("array", () => process.loadEnvFile([] as any));
run("symbol", () => process.loadEnvFile(Symbol("x") as any));

process.chdir(before);
