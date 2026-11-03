import { open } from "node:fs/promises";
import { writeFileSync, unlinkSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
const path = join(tmpdir(), `perry-nmi-handle-${process.pid}`);
writeFileSync(path, "first\nsecond\n");
const makeHandle = open;
const handle = await makeHandle(path, "r");
try {
    const lines = [];
    for await (const line of handle.readLines()) lines.push(line);
    console.log(lines.join("|"));
} finally { await handle.close(); unlinkSync(path); }
