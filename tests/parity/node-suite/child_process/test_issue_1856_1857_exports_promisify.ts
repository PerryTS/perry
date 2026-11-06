// Issue #1856: `child_process.ChildProcess` reads as `[Function: ChildProcess]`
// (previously the value-read tripped the #463 unimplemented-surface guard at
// compile time); `child_process.Stream` reads as `undefined` (Node does not
// export it).
//
// Issue #1857: `util.promisify(child_process.exec)` / `promisify(execFile)`
// return a function whose Promise resolves to `{ stdout, stderr }` (Node's
// custom-promisify shape), instead of `undefined`.
//
// Byte-for-byte vs `node --experimental-strip-types`.
import * as child_process from "node:child_process";
import { promisify } from "node:util";

// ── #1856: named exports ──
console.log("ChildProcess:", child_process.ChildProcess);
console.log("typeof ChildProcess:", typeof child_process.ChildProcess);
console.log("Stream:", child_process.Stream);
console.log("typeof Stream:", typeof child_process.Stream);

// ── #1857: promisify(exec) ──
const execP = promisify(child_process.exec);
console.log("typeof promisify(exec):", typeof execP);
const r1 = await execP("/bin/echo promisify-exec");
console.log("exec stdout:", String(r1.stdout).trim());

// ── #1857: promisify(execFile) ──
const execFileP = promisify(child_process.execFile);
console.log("typeof promisify(execFile):", typeof execFileP);
const r2 = await execFileP("/bin/echo", ["promisify-execFile"]);
console.log("execFile stdout:", String(r2.stdout).trim());
