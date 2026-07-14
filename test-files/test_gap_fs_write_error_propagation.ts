// `fs.write*` must distinguish a real OS write failure from a successful
// zero-byte write. A read-only fd is still open, so it bypasses EBADF
// preflight and exercises the syscall-error path.
import * as fs from "node:fs";

const dir = fs.mkdtempSync("perry-fs-write-error-");
const file = dir + "/file.txt";
fs.writeFileSync(file, "seed");
const fd = fs.openSync(file, "r");

function syncCode(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ": OK");
  } catch (err: any) {
    console.log(label + ": " + err.code + " " + err.syscall);
  }
}

function callbackCode(
  label: string,
  invoke: (callback: (err: any) => void) => void,
): Promise<void> {
  return new Promise((resolve) => {
    invoke((err: any) => {
      console.log(label + ": " + (err ? err.code + " " + err.syscall : "OK"));
      resolve();
    });
  });
}

async function main(): Promise<void> {
  try {
    syncCode("write string sync", () => fs.writeSync(fd, "x"));
    syncCode("write buffer sync", () => fs.writeSync(fd, Buffer.from("x"), 0, 1, null));
    await callbackCode("write string callback", (callback) => fs.write(fd, "x", callback));
    await callbackCode("write buffer callback", (callback) =>
      fs.write(fd, Buffer.from("x"), 0, 1, null, callback),
    );
  } finally {
    fs.closeSync(fd);
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

main();
