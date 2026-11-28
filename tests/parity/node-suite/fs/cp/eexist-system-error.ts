// cp onto an existing file throws Node's SystemError: own code, info, errno,
// syscall and path in that order, with the underlying EEXIST in `info`.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "perry-cp-eexist-"));
const src = path.join(dir, "src.txt");
const dst = path.join(dir, "dst.txt");
fs.writeFileSync(src, "a");
fs.writeFileSync(dst, "b");
try {
  fs.cpSync(src, dst, { force: false, errorOnExist: true });
} catch (error: any) {
  const scrub = (text: string) => text.split(dir).join("<dir>");
  console.log(error.name, error.code, Object.keys(error).join(","));
  console.log(scrub(JSON.stringify(error.info)));
  console.log(error.errno === error.info.errno, error.syscall, scrub(error.path));
} finally {
  fs.rmSync(dir, { recursive: true, force: true });
}
