import { AsyncLocalStorage } from "node:async_hooks";
import { close, fstat, open, read, writeFileSync, unlinkSync } from "node:fs";
const storage = new AsyncLocalStorage<string>();
const path = "/tmp/perry-async-hooks-fs-fd.txt";
writeFileSync(path, "fd-data");
await storage.run(
  "fs-fd",
  () =>
    new Promise<void>((resolve, reject) => {
      open(path, "r", (openError, fd) => {
        console.log("fs.open store:", storage.getStore());
        if (openError) return reject(openError);
        fstat(fd, (statError, stats) => {
          console.log("fs.fstat store:", storage.getStore(), stats.size);
          if (statError) return reject(statError);
          const buffer = Buffer.alloc(2);
          read(fd, buffer, 0, 2, 0, (readError, bytesRead) => {
            console.log(
              "fs.read store:",
              storage.getStore(),
              bytesRead,
              String(buffer),
            );
            if (readError) return reject(readError);
            close(fd, (closeError) => {
              console.log("fs.close store:", storage.getStore());
              if (closeError) return reject(closeError);
              resolve();
            });
          });
        });
      });
    }),
);
unlinkSync(path);
console.log("fs.fd outside:", String(storage.getStore()));
