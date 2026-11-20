// fs.write/fs.writeSync with an explicit `undefined` offset and length write
// the whole buffer (offset 0, length = rest of the buffer), as Effect's
// FileSystem does for every log line it flushes.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "ocrun7-")), "out.txt");
const bytes = new TextEncoder().encode("hello\n");
const fd = fs.openSync(file, "w");
fs.write(fd, bytes, undefined, undefined, undefined, (error, written) => {
  console.log("write u8", error, written);
  fs.write(fd, Buffer.from("buffer\n"), null, undefined, null, (error2, written2) => {
    console.log("write buffer", error2, written2);
    console.log("writeSync", fs.writeSync(fd, bytes, undefined, undefined, undefined));
    console.log("writeSync offset", fs.writeSync(fd, Buffer.from("xxtail\n"), 2, undefined, null));
    fs.closeSync(fd);
    console.log(JSON.stringify(fs.readFileSync(file, "utf8")));
    const rfd = fs.openSync(file, "r");
    const target = Buffer.alloc(5);
    console.log("readSync", fs.readSync(rfd, target, null, 5, null), target.toString());
    fs.closeSync(rfd);
    fs.rmSync(path.dirname(file), { recursive: true });
  });
});
