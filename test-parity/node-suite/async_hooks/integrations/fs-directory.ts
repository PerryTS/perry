import { AsyncLocalStorage } from "node:async_hooks";
import { mkdir, readdir, realpath, realpathSync, rmdir, rmSync } from "node:fs";
const storage = new AsyncLocalStorage<string>();
const path = `${realpathSync("/tmp")}/perry-async-hooks-fs-directory`;
rmSync(path, { recursive: true, force: true });
await storage.run(
  "fs-directory",
  () =>
    new Promise<void>((resolve, reject) => {
      mkdir(path, (mkdirError) => {
        console.log("fs.mkdir store:", storage.getStore());
        if (mkdirError) return reject(mkdirError);
        readdir(path, (readdirError, entries) => {
          console.log("fs.readdir store:", storage.getStore(), entries.length);
          if (readdirError) return reject(readdirError);
          realpath(path, (realpathError, resolved) => {
            console.log(
              "fs.realpath store:",
              storage.getStore(),
              resolved === path,
            );
            if (realpathError) return reject(realpathError);
            rmdir(path, (rmdirError) => {
              console.log("fs.rmdir store:", storage.getStore());
              if (rmdirError) return reject(rmdirError);
              resolve();
            });
          });
        });
      });
    }),
);
console.log("fs.directory outside:", String(storage.getStore()));
