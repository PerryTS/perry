// fs.read callback forms: (fd, options, cb) with a bigint or number position,
// (fd, buffer, offset, length, bigintPosition, cb), also through a function
// value (promisify-style helpers call fs.read indirectly).
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "perry-read-")), "r.txt");
fs.writeFileSync(file, "hello world");
const call = (f: Function, ...a: any[]) =>
  new Promise<any[]>((resolve) => f(...a, (e: any, n: any) => resolve([e, n])));
const [, fd] = await call(fs.open, file, "r", undefined);
const a = Buffer.alloc(5);
const [e1, n1] = await call(fs.read, fd, { buffer: a, position: BigInt(0) });
console.log(e1?.code ?? null, n1, a.toString("utf8", 0, n1));
const b = Buffer.alloc(5);
const [e2, n2] = await call(fs.read, fd, { buffer: b, position: 6 });
console.log(e2?.code ?? null, n2, b.toString("utf8", 0, n2));
const c = Buffer.alloc(3);
const [e3, n3] = await call(fs.read, fd, c, 0, 3, BigInt(1));
console.log(e3?.code ?? null, n3, c.toString());
const [e4, st] = await call(fs.fstat, fd, { bigint: true });
console.log(e4?.code ?? null, typeof st.size, String(st.size));
fs.closeSync(fd);
