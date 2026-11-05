// The listeners and the signal stream sit on descriptors 3 and up, where
// wasi-libc looks for pre-opened directories. It does so on the first path
// operation, so the program makes one. The host answers EBADF for all of them,
// so the scan ends at once and the program runs.
import * as fs from "node:fs";
import * as net from "node:net";

console.log(fs.existsSync("no-such-file"));
const server = net.createServer(() => {});
server.listen(0, () => {
  console.log("up");
  server.close();
});
