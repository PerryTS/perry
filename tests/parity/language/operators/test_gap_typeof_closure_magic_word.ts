// #10956: `typeof` intermittently answered "function" for an fs Error.
//
// `typeof` used to decide "function" from four bytes alone: CLOSURE_MAGIC
// ("CLOS", 0x434C4F53) read at the closure tag offset (12 on 64-bit). Arena
// slots are recycled without zeroing, and that offset is:
//
//   * padding in an ErrorHeader, which `alloc_error` never wrote. A 7-capture
//     closure is a 72-byte payload, the same size as an ErrorHeader, so an
//     Error born where such a closure died kept its magic;
//   * the high word of element 0 in an array, so it is user data;
//   * bytes 4..8 of a Buffer, so it is user data.
//
// OpenCode's lock helper narrows a caught value with
// `typeof err !== "object"` before reading `.code`. The misread made it
// rethrow an EEXIST it exists to swallow, and the TUI painted nothing.
//
// Every value goes through `opaque` so no `typeof` folds at compile time.

import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function opaque(x: unknown): unknown {
  return x;
}

// Deterministic: user data that spells the magic at the tag offset.
// 15937034497556480 === f64 bits 0x434C4F53_00000000.
console.log("array:", typeof opaque([15937034497556480]));
console.log("buffer:", typeof opaque(Buffer.from([0, 0, 0, 0, 0x53, 0x4f, 0x4c, 0x43])));
console.log("closure:", typeof opaque(() => 1));

// The OpenCode shape: dead 7-capture closures between fs errors, so the
// allocator recycles their slots for the ErrorHeaders.
function code(err: unknown) {
  if (typeof err !== "object" || err === null || !("code" in err)) return;
  const value = (err as { code: unknown }).code;
  if (typeof value !== "string") return;
  return value;
}

function mk(a: number, b: number, c: number, d: number, e: number, f: number, g: number) {
  return () => a + b + c + d + e + f + g;
}

const dir = mkdtempSync(join(tmpdir(), "perry-10956-"));
const lock = join(dir, "held.lock");
mkdirSync(lock);
let swallowed = 0;
let rethrown = 0;
let notObject = 0;
for (let round = 0; round < 20; round++) {
  let churn: unknown[] = [];
  for (let i = 0; i < 20000; i++) churn.push(mk(i, i, i, i, i, i, i));
  churn = [];
  for (let i = 0; i < 1000; i++) {
    try {
      mkdirSync(lock);
    } catch (err) {
      if (typeof err !== "object") notObject++;
      if (code(err) === "EEXIST") swallowed++;
      else rethrown++;
    }
  }
}
rmSync(dir, { recursive: true });
console.log("EEXIST swallowed:", swallowed, "rethrown:", rethrown);
console.log("errors whose typeof is not object:", notObject);
