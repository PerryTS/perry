import * as fs from "node:fs";
import { pathToFileURL } from "node:url";

const ROOT = "/tmp/perry_node_suite_fs_pathlike_file_url_validation";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const file = ROOT + "/file url.txt";
const fileUrl = pathToFileURL(file);
fs.writeFileSync(fileUrl, "url-data");
console.log("pathToFileURL read:", fs.readFileSync(fileUrl, "utf8"));
console.log("pathToFileURL stat:", fs.statSync(fileUrl).isFile());
console.log("pathToFileURL readdir:", fs.readdirSync(pathToFileURL(ROOT)).includes("file url.txt"));
console.log("pathToFileURL realpath suffix:", fs.realpathSync(fileUrl).endsWith("file url.txt"));
const made = fs.mkdtempSync(pathToFileURL(ROOT + "/urltmp-"));
console.log("pathToFileURL mkdtemp prefix:", made.startsWith(ROOT + "/urltmp-"));
console.log("buffer path read:", fs.readFileSync(Buffer.from(file), "utf8"));

function captureCode(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (err: any) {
    console.log(label, "code", err && err.code);
    console.log(label, "name", err && err.name);
  }
}

captureCode("encoded slash URL", () => fs.statSync(new URL("file://" + ROOT + "/bad%2Fname")));
captureCode("null byte URL", () => fs.statSync(new URL("file://" + ROOT + "/bad%00name")));
captureCode("null byte string", () => fs.statSync(ROOT + "/bad\0name"));
