// fs.writeSync(fd, buffer, offset, length) writes `length` bytes from
// `offset` of the buffer at the file's current position, and
// fs.writeSync(fd, buffer, offset) writes the rest from `offset`. The
// three- and four-argument buffer forms were read as the string form
// (third argument a file position): every write landed at that position and
// ignored the length. Checked through a named import and through the module
// object (a dynamic call), with Uint8Array views.
import { closeSync, openSync, readFileSync, unlinkSync, writeSync } from "node:fs";

const fs: any = (globalThis as any).process.getBuiltinModule("node:fs");
const big = new Uint8Array(10000);
for (let i = 0; i < big.length; i++) big[i] = (i % 250) + 1;
const pieces = [big.subarray(0, 3168), big.subarray(3168, 7000), big.subarray(7000)];

function check(label: string, write: (fd: number, chunk: Uint8Array, at: number, len: number) => number) {
    const path = `/tmp/perry-write-sync-${process.pid}-${label}.bin`;
    const fd = openSync(path, "w");
    for (const chunk of pieces) {
        let at = 0;
        while (at < chunk.length) at += write(fd, chunk, at, Math.min(1000, chunk.length - at));
    }
    closeSync(fd);
    const back = readFileSync(path);
    let diff = 0;
    for (let i = 0; i < big.length; i++) if (back[i] !== big[i]) diff++;
    console.log(label, "size", back.length, "diff", diff);
    unlinkSync(path);
}

check("named-4", (fd, chunk, at, len) => writeSync(fd, chunk, at, len));
check("module-4", (fd, chunk, at, len) => fs.writeSync(fd, chunk, at, len));
check("module-3", (fd, chunk, at) => fs.writeSync(fd, chunk, at));
check("named-3", (fd, chunk, at) => writeSync(fd, chunk, at));

const path = `/tmp/perry-write-sync-${process.pid}-string.txt`;
const fd = openSync(path, "w");
writeSync(fd, "hello world");
fs.writeSync(fd, "J", 0);
fs.writeSync(fd, "W", 6, "utf8");
closeSync(fd);
console.log("string", readFileSync(path, "utf8"));
unlinkSync(path);
