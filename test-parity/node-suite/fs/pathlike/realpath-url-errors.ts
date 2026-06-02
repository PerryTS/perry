import * as fs from "node:fs";
import { pathToFileURL } from "node:url";

const ROOT = "/tmp/perry_node_suite_fs_realpath_url_errors";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const file = ROOT + "/target.txt";
const missing = ROOT + "/missing.txt";
fs.writeFileSync(file, "ok");

function syncError(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + " code:", "no-error");
  } catch (err: any) {
    console.log(label + " code:", err?.code);
    console.log(label + " syscall type:", typeof err?.syscall);
    console.log(label + " path type:", typeof err?.path);
  }
}

function codeOf(fn: () => unknown) {
  try {
    fn();
    return "no-error";
  } catch (err: any) {
    return err?.code || "no-code";
  }
}

syncError("sync missing", () => fs.realpathSync(missing));

await new Promise<void>((resolve) => {
  fs.realpath(missing, (err, resolved) => {
    console.log("callback missing code:", err?.code);
    console.log("callback missing syscall type:", typeof err?.syscall);
    console.log("callback missing path type:", typeof err?.path);
    console.log("callback missing resolved undefined:", resolved === undefined);
    resolve();
  });
});

console.log("sync buffer valid:", fs.realpathSync(Buffer.from(file)).endsWith("/target.txt"));
console.log("sync file url valid:", fs.realpathSync(pathToFileURL(file)).endsWith("/target.txt"));

await new Promise<void>((resolve) => {
  fs.realpath(pathToFileURL(file), (err, resolved) => {
    console.log("callback file url err null:", err === null);
    console.log("callback file url valid:", resolved.endsWith("/target.txt"));
    resolve();
  });
});

console.log("sync http url code:", codeOf(() => fs.realpathSync(new URL("http://example.com/x"))));
console.log("sync hosted file url code:", codeOf(() => fs.realpathSync(new URL("file://example.com/tmp/x"))));
console.log("sync encoded slash url code:", codeOf(() => fs.realpathSync(new URL("file:///tmp/a%2Fb"))));
console.log("callback http url code:", codeOf(() => fs.realpath(new URL("http://example.com/x"), () => {})));
console.log("callback hosted file url code:", codeOf(() => fs.realpath(new URL("file://example.com/tmp/x"), () => {})));
console.log("callback encoded slash url code:", codeOf(() => fs.realpath(new URL("file:///tmp/a%2Fb"), () => {})));
