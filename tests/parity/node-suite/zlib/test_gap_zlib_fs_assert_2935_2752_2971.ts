// Parity coverage for three node-module behaviors:
//   #2935 — node:zlib gzipSync/deflateSync honor the { level } option;
//           an invalid level throws RangeError.
//   #2752 — a closed FileHandle's truncate() rejects with code "EBADF".
//   #2971 — assert.doesNotThrow re-throws a non-matching error AS the
//           original error type (not wrapped in AssertionError).

import zlib from "node:zlib";
import { writeFile, open } from "node:fs/promises";
import assert from "node:assert";

async function main() {
  // ── #2935 zlib level option ───────────────────────────────────────────
  const input = Buffer.from("a".repeat(4096));

  const gzip1 = zlib.gzipSync(input, { level: 1 });
  const gzip9 = zlib.gzipSync(input, { level: 9 });
  console.log("gzip level differs:", gzip1.length !== gzip9.length);

  const deflate1 = zlib.deflateSync(input, { level: 1 });
  const deflate9 = zlib.deflateSync(input, { level: 9 });
  console.log("deflate level differs:", deflate1.length !== deflate9.length);

  // valid output still round-trips (decompressed byte length matches input)
  console.log("gzip roundtrip:", zlib.gunzipSync(gzip9).length === input.length);
  console.log("deflate roundtrip:", zlib.inflateSync(deflate9).length === input.length);

  // default (no options) still works
  console.log("gzip default ok:", zlib.gunzipSync(zlib.gzipSync(input)).length === input.length);

  // invalid level throws RangeError before compression
  try {
    zlib.gzipSync("x", { level: 99 });
    console.log("level 99 NO throw");
  } catch (e: any) {
    console.log("level 99 threw:", e.name);
  }

  // ── #2752 closed FileHandle truncate -> EBADF ─────────────────────────
  await writeFile("/tmp/perry_zfa_test.txt", "abc");
  const fh = await open("/tmp/perry_zfa_test.txt", "r+");
  await fh.close();
  try {
    await fh.truncate(1);
    console.log("truncate NO reject");
  } catch (e: any) {
    console.log("truncate rejected code:", e.code);
  }

  // ── #2971 assert.doesNotThrow re-throws non-matching error ────────────
  try {
    assert.doesNotThrow(() => {
      throw new TypeError("bad");
    }, TypeError);
    console.log("dnt match NO throw");
  } catch (e: any) {
    console.log("dnt matching throw:", e.name, e.code);
  }
  try {
    assert.doesNotThrow(() => {
      throw new RangeError("bad");
    }, TypeError);
    console.log("dnt nonmatch NO throw");
  } catch (e: any) {
    console.log("dnt nonmatching throw:", e.name, e.code, e.message);
  }
}

main();
