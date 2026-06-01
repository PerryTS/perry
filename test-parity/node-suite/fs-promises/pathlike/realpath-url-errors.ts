import * as fs from "node:fs";
import * as fsp from "node:fs/promises";
import { pathToFileURL } from "node:url";

const ROOT = "/tmp/perry_node_suite_fsp_realpath_url_errors";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const file = ROOT + "/target.txt";
const missing = ROOT + "/missing.txt";
fs.writeFileSync(file, "ok");

async function promiseError(label: string, value: unknown) {
  try {
    await fsp.realpath(value as any);
    console.log(label + " code:", "no-error");
  } catch (err: any) {
    console.log(label + " code:", err?.code);
    console.log(label + " syscall type:", typeof err?.syscall);
    console.log(label + " path type:", typeof err?.path);
  }
}

await promiseError("promises missing", missing);

console.log("promises buffer valid:", (await fsp.realpath(Buffer.from(file))).endsWith("/target.txt"));
console.log("promises file url valid:", (await fsp.realpath(pathToFileURL(file))).endsWith("/target.txt"));

await promiseError("promises http url", new URL("http://example.com/x"));
await promiseError("promises hosted file url", new URL("file://example.com/tmp/x"));
await promiseError("promises encoded slash url", new URL("file:///tmp/a%2Fb"));
