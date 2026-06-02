import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_fd_read_write_object_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const p = ROOT + "/file.txt";
fs.writeFileSync(p, "abcdef");

await new Promise<void>((resolve) => {
  fs.open(p, "r", (openErr, fd) => {
    const buf = Buffer.alloc(6, "_");
    fs.read(fd, buf, { offset: 1, length: 3, position: 2 }, (err, bytesRead, sameBuffer) => {
      console.log("read object open err null:", openErr === null);
      console.log("read object err null:", err === null);
      console.log("read object bytes:", bytesRead);
      console.log("read object same buffer:", sameBuffer === buf);
      console.log("read object content:", buf.toString("utf8"));
      fs.closeSync(fd);
      resolve();
    });
  });
});

const out = ROOT + "/out.txt";
await new Promise<void>((resolve) => {
  fs.open(out, "w+", (openErr, fd) => {
    const buf = Buffer.from("xyz123");
    fs.write(fd, buf, { offset: 1, length: 3, position: 0 }, (err, bytesWritten, sameBuffer) => {
      console.log("write object open err null:", openErr === null);
      console.log("write object err null:", err === null);
      console.log("write object bytes:", bytesWritten);
      console.log("write object same buffer:", sameBuffer === buf);
      fs.closeSync(fd);
      console.log("write object content:", fs.readFileSync(out, "utf8"));
      resolve();
    });
  });
});
