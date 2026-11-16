// fs.readSync(fd, buffer) reads up to the whole buffer from the current file
// position, both as a direct call and through a function value.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "perry-readsync-")), "r.txt");
fs.writeFileSync(file, "hello world!");

const fd = fs.openSync(file, "r");
const first = Buffer.alloc(5);
console.log("direct", fs.readSync(fd, first), first.toString());
const readSync: any = fs.readSync;
const second = Buffer.alloc(4);
console.log("value", readSync(fd, second), second.toString());
const rest = Buffer.alloc(8);
const n = fs.readSync(fd, rest);
console.log("rest", n, rest.toString("utf8", 0, n));
fs.closeSync(fd);
fs.rmSync(path.dirname(file), { recursive: true, force: true });
