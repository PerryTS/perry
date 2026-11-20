// fs.readdirSync / fs.promises.readdir / fs.readdir on a directory that does
// not exist, or on a file, must fail with Node's scandir error (ENOENT /
// ENOTDIR), not return an empty array. Next.js's recursiveReadDir rethrows
// unless `err.code === 'ENOENT'`; an empty array read as an existing `static`
// directory and printed the static-dir deprecation warning.
import * as fs from "fs";
import * as fsp from "fs/promises";
import * as os from "os";
import * as path from "path";

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "perry-readdir-"));
const file = path.join(dir, "f.txt");
fs.writeFileSync(file, "x");
fs.mkdirSync(path.join(dir, "sub"));
const missing = path.join(dir, "nope");

function sync(name: string, f: () => unknown): void {
  try {
    console.log(name, JSON.stringify(f()));
  } catch (e: any) {
    console.log(name, "threw", e.code, e.syscall, e.message.replace(dir, "<dir>"));
  }
}

sync("sync-ok", () => fs.readdirSync(dir).sort());
sync("sync-types", () => fs.readdirSync(dir, { withFileTypes: true }).map((d) => d.name + ":" + d.isDirectory()).sort());
sync("sync-missing", () => fs.readdirSync(missing));
sync("sync-missing-types", () => fs.readdirSync(missing, { withFileTypes: true }));
sync("sync-file", () => fs.readdirSync(file));
sync("sync-bad-arg", () => fs.readdirSync(null as any));

async function main(): Promise<void> {
  for (const [name, p, opts] of [
    ["p-ok", dir, undefined],
    ["p-missing", missing, undefined],
    ["p-missing-types", missing, { withFileTypes: true }],
    ["p-file", file, undefined],
  ] as const) {
    try {
      const r = await fsp.readdir(p, opts as any);
      console.log(name, JSON.stringify((r as any[]).map(String).sort()));
    } catch (e: any) {
      console.log(name, "rejected", e.code, e.syscall);
    }
  }
  const all = await Promise.all(
    [dir, missing].map(async (d) => {
      try {
        return await fsp.readdir(d, { withFileTypes: true });
      } catch (err: any) {
        if (err.code !== "ENOENT") throw err;
        return null;
      }
    }),
  );
  console.log("all", all.map((r) => (r === null ? "null" : r.length)).join(","));
  for (const [name, p] of [["cb-missing", missing], ["cb-file", file], ["cb-ok", dir]]) {
    await new Promise<void>((resolve) => {
      fs.readdir(p, (err, files) => {
        console.log(name, err ? "err " + err.code + " " + err.syscall : JSON.stringify(files.sort()));
        resolve();
      });
    });
  }
  fs.rmSync(dir, { recursive: true });
}
main();
