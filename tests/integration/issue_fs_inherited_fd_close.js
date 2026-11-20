// closeSync on an inherited descriptor closes that descriptor, as in Node: a
// peer reading the other end of the pipe sees EOF while this process is alive.
//
// The harness starts the program with fd 4 as the write end of a fifo whose
// reader creates the file named by argv[2] once it has read EOF.

import { closeSync, existsSync } from "fs";

const marker = process.argv[2];
closeSync(4);

const deadline = Date.now() + 15000;
while (!existsSync(marker) && Date.now() < deadline) {
  // spin until the reader reports EOF
}
if (!existsSync(marker)) {
  process.stderr.write("peer never saw EOF after closeSync(4)\n");
  process.exit(1);
}
process.stdout.write("eof\n");
