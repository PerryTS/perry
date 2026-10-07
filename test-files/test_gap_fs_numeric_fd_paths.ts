// A numeric fd must be classified as an fd before any header/byte probe:
// appendFileSync / writeFileSync / readFileSync with a number must not treat
// its bits as a Buffer or URL pointer.
import * as fs from "fs";
import * as os from "os";
import * as path from "path";

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "fdpaths-"));
const p = path.join(dir, "x.txt");

let fd = fs.openSync(p, "a");
fs.appendFileSync(fd, "one");
fs.appendFileSync(fd, Buffer.from("-two"));
fs.appendFileSync(fd, "-three", "utf8");
fs.closeSync(fd);
console.log(fs.readFileSync(p, "utf8"));

fd = fs.openSync(p, "w");
fs.writeFileSync(fd, "written");
fs.writeFileSync(fd, Buffer.from("+buf"));
fs.closeSync(fd);
console.log(fs.readFileSync(p, "utf8"));

fd = fs.openSync(p, "r");
console.log(fs.readFileSync(fd, "utf8"));
fs.closeSync(fd);

// several fd magnitudes, including ones that decode into odd address bands
const fds: number[] = [];
for (let i = 0; i < 40; i++) fds.push(fs.openSync(p, "a"));
for (const f of fds) fs.appendFileSync(f, ".");
for (const f of fds) fs.closeSync(f);
console.log(fs.readFileSync(p, "utf8").length);

for (const bad of [123456, 2147483647]) {
  try { fs.appendFileSync(bad, "x"); } catch (e: any) { console.log(e.code); }
  try { fs.readFileSync(bad); } catch (e: any) { console.log(e.code); }
}
fs.rmSync(dir, { recursive: true });
