import { readFile, writeFile, unlink } from "node:fs/promises";
import * as fsp from "node:fs/promises";
import { promises, constants } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
const path = join(tmpdir(), `perry-nmi-promises-${process.pid}`);
console.log("identity", readFile === fsp.readFile, readFile === promises.readFile);
const read = readFile;
await writeFile(path, "selected promise export\n");
try {
    console.log((await read(path, "utf8")).trim());
    console.log((await [readFile].map(fn => fn(path, "utf8"))[0]).trim());
    const key = "F_OK";
    console.log(constants[key] === fsp.constants[key]);
} finally { await unlink(path); }
