// Gap test: #10452 / #10451 — node:fs read failures must surface Node's error.
//
// #10452: the Buffer-mode reads (no encoding) swallowed the failure.
// `fs.readFileSync(missing)` returned null, `readFileSync(missing, {})`
// returned undefined and `fs.promises.readFile(missing)` resolved undefined,
// so `try { readFileSync(optional) } catch { defaults }` took the wrong
// branch. A directory read succeeded or reported `open` instead of Node's
// `EISDIR ... read`.
// #10451: a `fs.createReadStream` open/read failure emitted a bare Error with
// no code/errno/syscall/path and the Rust `(os error N)` message text, and a
// failed stream handed to `fs.promises.writeFile` reported EBADF for its
// missing fd instead of the constructor's failure.
//
// Every failure prints code/errno/syscall/path/message and the own-key order;
// the successful reads are the controls.
import * as fs from "node:fs";
import fsDefault from "node:fs";
import { readFileSync } from "node:fs";
import * as fsp from "node:fs/promises";
import { readFile as readFileP } from "node:fs/promises";

const base = "/tmp/perry_gap_10452_fs_read_error_shapes";
fs.rmSync(base, { recursive: true, force: true });
fs.mkdirSync(base + "/dir", { recursive: true });
const missing = base + "/missing.txt";
const missingParent = base + "/no-such-dir/child.txt";
const dir = base + "/dir";
const ok = base + "/ok.txt";
fs.writeFileSync(ok, "hello");

function shape(e: any): string {
  const fields = { code: e.code, errno: e.errno, syscall: e.syscall, path: e.path, message: e.message };
  return JSON.stringify(fields) + " keys=" + Object.keys(e).join(",") + " isError=" + (e instanceof Error);
}

function show(v: any): string {
  if (Buffer.isBuffer(v)) return "Buffer<" + v.toString() + ">";
  return typeof v + " " + String(v);
}

function sync(label: string, f: () => any): void {
  try {
    console.log(label, "returned", show(f()));
  } catch (e: any) {
    console.log(label, "threw", shape(e));
  }
}

async function promised(label: string, f: () => Promise<any>): Promise<void> {
  try {
    console.log(label, "resolved", show(await f()));
  } catch (e: any) {
    console.log(label, "rejected", shape(e));
  }
}

function callback(label: string, start: (cb: (err: any, data?: any) => void) => void): Promise<void> {
  return new Promise((resolve) => {
    start((err: any, data?: any) => {
      if (err) console.log(label, "err", shape(err), "data", String(data));
      else console.log(label, "err", String(err), "data", show(data));
      resolve();
    });
  });
}

function readStream(label: string, target: string): Promise<void> {
  return new Promise((resolve) => {
    const s = fs.createReadStream(target);
    const chunks: string[] = [];
    s.on("error", (e: any) => {
      console.log(label, "error", shape(e));
      resolve();
    });
    s.on("data", (chunk: any) => chunks.push(String(chunk)));
    s.on("end", () => {
      console.log(label, "end", chunks.join(""));
      resolve();
    });
  });
}

// --- sync ---
sync("readFileSync(missing)", () => fs.readFileSync(missing));
sync("readFileSync(missing, {})", () => fs.readFileSync(missing, {}));
sync("readFileSync(missing, {flag:'r'})", () => fs.readFileSync(missing, { flag: "r" }));
sync("readFileSync(missing, 'utf8')", () => fs.readFileSync(missing, "utf8"));
sync("readFileSync(missing, {encoding})", () => fs.readFileSync(missing, { encoding: "utf8" }));
sync("readFileSync(missing, 'latin1')", () => fs.readFileSync(missing, "latin1"));
sync("readFileSync(missingParent, {flag:'a+'})", () => fs.readFileSync(missingParent, { flag: "a+" }));
sync("named readFileSync(missing)", () => readFileSync(missing));
sync("default fs.readFileSync(missing)", () => fsDefault.readFileSync(missing));
sync("readFileSync(dir)", () => fs.readFileSync(dir));
sync("readFileSync(dir, 'utf8')", () => fs.readFileSync(dir, "utf8"));
const dirFd = fs.openSync(dir, "r");
sync("readFileSync(dirFd)", () => fs.readFileSync(dirFd));
fs.closeSync(dirFd);
sync("readFileSync(ok)", () => fs.readFileSync(ok));
sync("readFileSync(ok, {})", () => fs.readFileSync(ok, {}));
sync("readFileSync(ok, 'utf8')", () => fs.readFileSync(ok, "utf8"));
sync("named readFileSync(ok)", () => readFileSync(ok));
sync("default fs.readFileSync(ok)", () => fsDefault.readFileSync(ok));
const okFd = fs.openSync(ok, "r");
sync("readFileSync(okFd)", () => fs.readFileSync(okFd));
fs.closeSync(okFd);
let config: any;
try {
  config = fs.readFileSync(missing);
} catch {
  config = "defaults";
}
console.log("optional-file fallback:", show(config));

async function main(): Promise<void> {
  // --- callback ---
  await callback("readFile(missing, cb)", (cb) => fs.readFile(missing, cb));
  await callback("readFile(missing, {}, cb)", (cb) => fs.readFile(missing, {}, cb));
  await callback("readFile(missing, 'utf8', cb)", (cb) => fs.readFile(missing, "utf8", cb));
  await callback("readFile(dir, cb)", (cb) => fs.readFile(dir, cb));
  await callback("readFile(dir, 'utf8', cb)", (cb) => fs.readFile(dir, "utf8", cb));
  await callback("readFile(ok, cb)", (cb) => fs.readFile(ok, cb));
  await callback("readFile(ok, 'utf8', cb)", (cb) => fs.readFile(ok, "utf8", cb));

  // --- promises ---
  await promised("fs.promises.readFile(missing)", () => fs.promises.readFile(missing));
  await promised("fsp.readFile(missing, {})", () => fsp.readFile(missing, {}));
  await promised("fsp.readFile(missing, 'utf8')", () => fsp.readFile(missing, "utf8"));
  await promised("named readFile(missing)", () => readFileP(missing));
  await promised("fsp.readFile(dir)", () => fsp.readFile(dir));
  await promised("fsp.readFile(dir, 'utf8')", () => fsp.readFile(dir, "utf8"));
  await promised("fsp.readFile(ok)", () => fsp.readFile(ok));
  await promised("named readFile(ok, 'utf8')", () => readFileP(ok, "utf8"));
  const missingIsEnoent = await fsp.readFile(missing).then(
    () => false,
    (e: any) => e.code === "ENOENT",
  );
  console.log("readFile(missing).catch sees ENOENT:", missingIsEnoent);

  // --- FileHandle ---
  await promised("fsp.open(missing)", () => fsp.open(missing));
  const dirHandle = await fsp.open(dir);
  await promised("dirHandle.readFile()", () => dirHandle.readFile());
  await dirHandle.close();
  const okHandle = await fsp.open(ok);
  await promised("okHandle.readFile()", () => okHandle.readFile());
  await okHandle.close();

  // --- streams ---
  await readStream("createReadStream(missing)", missing);
  await readStream("createReadStream(dir)", dir);
  await readStream("createReadStream(ok)", ok);
  await new Promise<void>((resolve) => {
    fs.createWriteStream(missingParent).on("error", (e: any) => {
      console.log("createWriteStream(missingParent) error", shape(e));
      resolve();
    });
  });

  // A read stream consumed by fs.promises.writeFile must report the failure the
  // constructor saw, not a later EBADF for its missing fd. Node's callback and
  // sync writeFile reject a stream outright (ERR_INVALID_ARG_TYPE), so only the
  // promise form is covered; the `'error'` listener keeps the failure handled,
  // which is what makes the ordering deterministic.
  const handled = (target: string) => {
    const stream = fs.createReadStream(target);
    stream.on("error", () => {});
    return stream;
  };
  await promised("writeFile(out, readStream(missing))", () =>
    fsp.writeFile(base + "/copy-missing.txt", handled(missing)),
  );
  await promised("writeFile(out, readStream(dir))", () =>
    fsp.writeFile(base + "/copy-dir.txt", handled(dir)),
  );
  await promised("writeFile(out, readStream(ok))", () =>
    fsp.writeFile(base + "/copy-ok.txt", handled(ok)),
  );
  console.log("copy-ok.txt:", show(fs.readFileSync(base + "/copy-ok.txt")));

  fs.rmSync(base, { recursive: true, force: true });
}

main();
