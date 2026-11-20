// Descriptors above stderr that the process inherited (a parent's `stdio[3]`
// pipe, a shell's `3<file`) are usable by number, as in Node.
//
// The harness starts the program with `3<` and `7<` redirections. Only 0..2
// were registered, so every fs op on fd 3 threw "EBADF: bad file descriptor".

import { readSync, fstatSync, openSync, closeSync, readFileSync } from "fs";

const buf = Buffer.alloc(64);
const n = readSync(3, buf, 0, 64, null);
process.stdout.write("fd3: " + buf.toString("utf8", 0, n));

const st = fstatSync(7);
if (!st.isFile()) {
  throw new Error("fstatSync(7) is not a file");
}

// A descriptor the program opens never takes an inherited number.
const own = openSync(process.argv[1], "r");
if (own === 3 || own === 7) {
  throw new Error(`openSync returned inherited descriptor ${own}`);
}
closeSync(own);

process.stdout.write("fd7: " + readFileSync(7, "utf8"));
process.stdout.write("done\n");
