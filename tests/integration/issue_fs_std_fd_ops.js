// fs operations addressing the process-standard descriptors (0/1/2) must work.
//
// Perry hands out its own fd ids from 100 up, and every fs entry point gates on
// membership in that registry. Nothing ever registered stdin/stdout/stderr, so
// `writeSync(1, ...)` — which Node writes straight to the terminal — threw
// "EBADF: bad file descriptor, write".

import { writeSync, readSync, fstatSync } from "fs";

const wrote = writeSync(1, "stdout-fd-write\n");
if (wrote !== 16) {
  throw new Error(`writeSync(1, string) returned ${wrote}, expected 16`);
}

const wroteErr = writeSync(2, "stderr-fd-write\n");
if (wroteErr !== 16) {
  throw new Error(`writeSync(2, string) returned ${wroteErr}, expected 16`);
}

const buf = Buffer.from("stdout-fd-buffer\n");
const wroteBuf = writeSync(1, buf);
if (wroteBuf !== buf.length) {
  throw new Error(`writeSync(1, buffer) returned ${wroteBuf}, expected ${buf.length}`);
}

// stdin is redirected from /dev/null by the harness: the read must reach EOF
// rather than fail the fd gate.
const sink = Buffer.alloc(8);
const read = readSync(0, sink, 0, 8, null);
if (read !== 0) {
  throw new Error(`readSync(0) returned ${read}, expected 0 at EOF`);
}

// fstat on a standard fd goes through the same gate.
const stats = fstatSync(1);
if (typeof stats.isFile !== "function") {
  throw new Error("fstatSync(1) did not return a Stats object");
}

process.stdout.write("done\n");
