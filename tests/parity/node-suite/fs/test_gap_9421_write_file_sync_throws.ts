// #9421 (sibling of the `appendFile` swallow): `fs.writeFileSync` must throw.
//
// `writeFileSync` had its own private copy of the write op inside
// `js_fs_write_file_sync_options`, which reported failure by RETURNING `0`.
// Every caller -- the codegen lowering, the namespace/computed dispatch entry
// -- discarded that status, so a failed write was reported as a success. The
// promise and callback forms were already correct: both run
// `write_file_path_or_fd_result`, which returns a Node-shaped fs error. The
// sync form now runs that same core, so all three agree.
//
// Routing it through the shared core fixes three divergences at once, because
// the private copy read its payload with `bytes_from_value` instead of
// `consume_write_file_input`:
//
//   * a failing write returned instead of throwing;
//   * `writeFileSync(p, 42)` wrote something instead of throwing
//     ERR_INVALID_ARG_TYPE;
//   * the `encoding` option was ignored -- `writeFileSync(p, "414243", "hex")`
//     wrote the six literal digits rather than the three bytes "ABC".
//
// The `mode` option is covered too: it was dropped on the create path, so
// every file Perry made landed `0666 & ~umask` where Node honours the request
// (`{ mode: 0o600 }` gave `0644`). It now reaches `open(2)`, which means it
// applies on create and is ignored for an existing file -- Node's rule, pinned
// below by chmod-ing a file and rewriting it with a mode.
//
// This test is umask-sensitive by construction: the assertions use 0o600
// (unaffected by the usual 0o022) and a plain-default probe, so it reads the
// same under any umask that does not strip owner bits.

import * as fs from "node:fs";
import { writeFileSync, mkdtempSync, rmSync, existsSync, statSync, chmodSync } from "node:fs";
import { writeFile as writeFileP, appendFile as appendFileP } from "node:fs/promises";
import { writeFile as writeFileCb } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = mkdtempSync(join(tmpdir(), "perry9421w-"));
const missing = join(root, "nodir", "out.txt");

function code(e: unknown): string {
  const c = (e as { code?: string } | null)?.code;
  return typeof c === "string" ? c : "NO-CODE";
}
function probe(name: string, fn: () => void): void {
  try {
    fn();
    console.log(name + ": no-throw");
  } catch (e) {
    console.log(name + ": " + code(e));
  }
}
function mode(p: string): string {
  return (statSync(p).mode & 0o777).toString(8);
}

async function main(): Promise<void> {
  // 1. a failing write must throw, in every call shape the lowering can take
  probe("sync/named", () => writeFileSync(missing, "x"));
  probe("sync/namespace", () => fs.writeFileSync(missing, "x"));
  const dyn = fs as unknown as Record<string, (...a: unknown[]) => unknown>;
  probe("sync/computed", () => dyn["writeFileSync"](missing, "x"));
  console.log("sync/created=" + existsSync(missing));

  // 2. the promise and callback forms already agreed; pin them as controls
  try {
    await writeFileP(missing, "x");
    console.log("promise: resolved");
  } catch (e) {
    console.log("promise: " + code(e));
  }
  await new Promise<void>((r) => {
    writeFileCb(missing, "x", (err) => {
      console.log("callback: " + (err ? code(err) : "no-err"));
      r();
    });
  });

  // 3. argument validation reached only through the shared core
  const p = join(root, "d.bin");
  probe("data/string", () => writeFileSync(p, "abc"));
  console.log("  size=" + statSync(p).size);
  probe("data/buffer", () => writeFileSync(p, Buffer.from([1, 2, 3, 4])));
  console.log("  size=" + statSync(p).size);
  probe("data/u8", () => writeFileSync(p, new Uint8Array([1, 2, 3])));
  console.log("  size=" + statSync(p).size);
  probe("data/u32", () => writeFileSync(p, new Uint32Array([1, 2])));
  console.log("  size=" + statSync(p).size);
  probe("data/dataview", () => writeFileSync(p, new DataView(new ArrayBuffer(5))));
  console.log("  size=" + statSync(p).size);
  probe("data/number", () => writeFileSync(p, 42 as unknown as string));
  probe("data/object", () => writeFileSync(p, { a: 1 } as unknown as string));
  probe("data/null", () => writeFileSync(p, null as unknown as string));

  // 4. the encoding option, as a bare string and inside the options object
  probe("enc/hex", () => writeFileSync(p, "414243", "hex"));
  console.log("  hex-read=" + fs.readFileSync(p, "utf8"));
  probe("enc/obj", () => writeFileSync(p, "QUJD", { encoding: "base64" }));
  console.log("  b64-read=" + fs.readFileSync(p, "utf8"));

  // 5. the flag option still selects append
  probe("flag/a", () => writeFileSync(p, "-tail", { flag: "a" }));
  console.log("  after-append=" + fs.readFileSync(p, "utf8"));

  // 6. mode on the create path, all four surfaces
  const m1 = join(root, "m1.txt");
  writeFileSync(m1, "x", { mode: 0o600 });
  console.log("mode/writeFileSync=" + mode(m1));
  const m2 = join(root, "m2.txt");
  await writeFileP(m2, "x", { mode: 0o600 });
  console.log("mode/promises.writeFile=" + mode(m2));
  const m3 = join(root, "m3.txt");
  await appendFileP(m3, "x", { mode: 0o600 });
  console.log("mode/promises.appendFile=" + mode(m3));
  const m4 = join(root, "m4.txt");
  fs.appendFileSync(m4, "x", { mode: 0o600 });
  console.log("mode/appendFileSync=" + mode(m4));

  // mode applies on CREATE only: an existing file keeps its permissions
  chmodSync(m1, 0o644);
  writeFileSync(m1, "y", { mode: 0o600 });
  console.log("mode/existing-file-kept=" + mode(m1));

  rmSync(root, { recursive: true, force: true });
}
main();
