// `openSync` must pass the flags it does not model through to open(2), and a
// failing `readSync` must surface the syscall error instead of reporting EOF.
//
// Perry translated only O_ACCMODE/O_CREAT/O_EXCL/O_TRUNC/O_APPEND into Rust's
// OpenOptions and dropped the rest, so O_NONBLOCK never reached open(2): the
// descriptor stayed blocking and a `readSync` with no data available hung
// forever where Node throws EAGAIN. `readSync` also mapped every error to 0,
// making a failure indistinguishable from a clean end-of-file.

import { openSync, readSync, closeSync, constants } from "fs";

// A FIFO has no writer, so a non-blocking read has nothing to deliver: Node
// answers EAGAIN. (A blocking descriptor would park here forever.)
import { mkdtempSync, rmSync } from "fs";
import { execFileSync } from "child_process";
import { tmpdir } from "os";
import { join } from "path";

const dir = mkdtempSync(join(tmpdir(), "perry-nonblock-"));

// The fifo and the descriptor must be released even when an assertion below
// throws, or a failing run leaves a temp directory (and an open fd) behind on
// every retry.
try {
  const fifo = join(dir, "fifo");
  execFileSync("mkfifo", [fifo]);

  const fd = openSync(fifo, constants.O_RDONLY | constants.O_NONBLOCK);
  try {
    const buf = Buffer.alloc(16);
    let code = null;
    try {
      readSync(fd, buf, 0, 16, null);
      throw new Error("readSync resolved instead of raising EAGAIN");
    } catch (err) {
      code = err.code;
    }

    if (code !== "EAGAIN") {
      throw new Error(`readSync on an empty non-blocking FIFO reported ${code}, expected EAGAIN`);
    }
  } finally {
    closeSync(fd);
  }
} finally {
  rmSync(dir, { recursive: true, force: true });
}

console.log("nonblocking-read ok");
