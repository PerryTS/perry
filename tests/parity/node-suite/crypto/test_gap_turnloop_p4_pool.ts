// turnloop P4 — CPU-bound native work must not run on the thread that owns the
// JS heap.
//
// Node runs `crypto.pbkdf2`, `crypto.scrypt` and the `zlib` one-shots on
// libuv's threadpool. Perry ran all three INLINE and deferred only the
// *callback*, so the API looked asynchronous while the work itself froze every
// timer, socket and immediate in the process. Measured on the oracle box before
// this test was written: a six-million-iteration `pbkdf2` cost **724 ms inside
// the call** on Perry and **0 ms** on Node 26.5.1; `scrypt` at p=16 cost 418 ms
// against 0 ms. turnloop P4 moves them to the shared bounded blocking pool.
//
// The discriminating quantity is how long the *synchronous call* takes — not a
// tick count, which does not discriminate at all: an inline implementation
// schedules its callback a turn later too, so both arms report "the loop turned
// afterwards". What separates them is that `pbkdf2(...)` returns immediately
// when the work went to a pool and only after ~a second when it did not.
//
// Only booleans and fixed digests are printed, never a duration or a count, so
// the output is machine-independent; the 250 ms threshold sits between the two
// measured populations with a 1.7x margin on the smaller of them.
//
// The digests are the second half of the contract: work that crosses to another
// thread and back must come back identical. They are Node 26.5.1's own output.
//
// @covers
// crates/perry-runtime/src/turnloop_pool/mod.rs
// crates/perry-stdlib/src/crypto/kdf.rs: js_crypto_pbkdf2_async_alg, js_crypto_scrypt_async
// crates/perry-stdlib/src/zlib.rs: queue_zlib_callback
import { pbkdf2, scrypt } from "node:crypto";
import { gzip, gunzip } from "node:zlib";
import { promisify } from "node:util";

const pbkdf2Async = promisify(pbkdf2);
const scryptAsync = promisify(scrypt);
const gzipAsync = promisify(gzip);
const gunzipAsync = promisify(gunzip);

// Comfortably below the 418 ms an inline `scrypt` costs here and far below the
// 724 ms an inline `pbkdf2` costs, while being ~250x what either costs when the
// work is on a pool.
const STALL_MS = 250;

async function callCost(
  start: () => Promise<unknown>,
): Promise<[boolean, unknown]> {
  const t0 = Date.now();
  const pending = start();
  const elapsed = Date.now() - t0;
  return [elapsed < STALL_MS, await pending];
}

async function main(): Promise<void> {
  // ── pbkdf2: six million iterations, ~0.7s of pure CPU ────────────────────
  const [pbkdfPrompt, pbkdfValue] = await callCost(() =>
    pbkdf2Async("perry", "turnloop", 6_000_000, 32, "sha256"),
  );
  console.log("pbkdf2 call returned without deriving:", pbkdfPrompt);
  console.log(
    "pbkdf2 digest:",
    (pbkdfValue as Buffer).toString("hex") ===
      "f5d5d2cedd1bd1269bf19ac2f752e5256f5559d78387c6ab4c6568067e7d712f",
  );

  // ── scrypt: memory-hard, p=16 so the inline cost is unambiguous ───────────
  const [scryptPrompt, scryptValue] = await callCost(() =>
    scryptAsync("perry", "turnloop", 64, { N: 16384, r: 8, p: 16 }),
  );
  console.log("scrypt call returned without deriving:", scryptPrompt);
  console.log(
    "scrypt digest prefix:",
    (scryptValue as Buffer).toString("hex").slice(0, 32) ===
      "355fd2b6b986eac2024bbff780cb83f3",
  );
  console.log("scrypt length:", (scryptValue as Buffer).length);

  // ── zlib: the codecs must survive the crossing byte for byte ─────────────
  //
  // Deliberately no timing assertion here and no comparison against
  // `gzipSync`: Perry's one-shot and sync gzip paths pick different
  // compression levels, so their outputs differ in length on both arms of this
  // change. That is a real divergence from Node, and a pre-existing one — it is
  // not what this test is about, and pinning it here would make a P4 fixture go
  // red for a P4-unrelated reason. Round-tripping is what the pool crossing can
  // break, so that is what is asserted.
  const payload = Buffer.alloc(4 * 1024 * 1024);
  for (let i = 0; i < payload.length; i++) payload[i] = i % 251;

  const packed = (await gzipAsync(payload)) as Buffer;
  console.log("gzip really compressed:", packed.length < payload.length);
  const unpacked = (await gunzipAsync(packed)) as Buffer;
  console.log("gunzip round trip is byte-identical:", unpacked.equals(payload));

  // Ten at once: the pool is bounded (four threads by default), so a burst has
  // to queue and every one of them must still come back with its own answer —
  // the failure mode a shared work queue has and a thread-per-job does not.
  const burst = await Promise.all(
    [1, 2, 3, 4, 5, 6, 7, 8, 9, 10].map(async (n) => {
      const chunk = Buffer.alloc(256 * 1024, n);
      const out = (await gunzipAsync(
        (await gzipAsync(chunk)) as Buffer,
      )) as Buffer;
      return (
        out.length === chunk.length && out[0] === n && out[out.length - 1] === n
      );
    }),
  );
  console.log("ten concurrent round trips all correct:", burst.every(Boolean));
  console.log("ten concurrent round trips count:", burst.length);

  // ── A failure still reaches the awaiter rather than hanging it ────────────
  let gunzipError = "none";
  try {
    await gunzipAsync(Buffer.from([0, 1, 2, 3, 4, 5, 6, 7]));
  } catch {
    gunzipError = "threw";
  }
  console.log("gunzip of garbage:", gunzipError);
}

main();
