// WASI has no dup(), and the fd registry only seeded descriptors 0..=2 under
// cfg(unix), so fs.writeSync(1, ...) and fs.write(1, ...) threw EBADF there. A
// logger's synchronous stdout destination writes exactly this way.
import * as fs from "node:fs";
console.log("sync string", fs.writeSync(1, "to fd 1\n"));
console.log("sync buffer", fs.writeSync(1, Buffer.from("buffer to fd 1\n")));
fs.write(1, "async to fd 1\n", (err, written) => {
  console.log("callback", err === null, written);
});
