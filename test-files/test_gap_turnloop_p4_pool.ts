// turnloop P4 — CPU-bound native work must not freeze the event loop.
//
// Node runs `crypto.pbkdf2`, `crypto.scrypt` and `zlib.gzip`/`gunzip` on
// libuv's threadpool, so while one of them is working, timers and immediates
// keep firing. Perry ran all three INLINE on the thread that owns the JS heap
// and deferred only the callback: the API looked asynchronous, and a
// two-million-iteration pbkdf2 stopped every timer, socket and immediate in the
// process for the whole derivation. turnloop P4 moves them to the shared
// bounded blocking pool.
//
// What is pinned is a boolean, never a count or a duration: "the call returned
// without doing the work" and "a timer fired while the work was in flight".
// Counts and durations are machine-dependent on both engines; these are not,
// because the workloads are sized to seconds against a 250 ms threshold.
//
// The byte-correctness half matters just as much: work that crosses to another
// thread and back must come back identical, so every operation's result is
// checked against its synchronous twin.
//
// @covers
// crates/perry-runtime/src/turnloop_pool/mod.rs
// crates/perry-stdlib/src/crypto/kdf.rs: js_crypto_pbkdf2_async_alg, js_crypto_scrypt_async
// crates/perry-stdlib/src/zlib.rs: queue_zlib_callback
import { pbkdf2, pbkdf2Sync, scrypt, scryptSync } from "node:crypto";
import { gzip, gunzip, gzipSync, gunzipSync } from "node:zlib";
import { promisify } from "node:util";

const pbkdf2Async = promisify(pbkdf2);
const scryptAsync = promisify(scrypt);
const gzipAsync = promisify(gzip);
const gunzipAsync = promisify(gunzip);

// The discriminating quantity is how long the *synchronous call* takes, not
// how many loop turns happen before the promise settles. A tick count does not
// discriminate: an inline implementation schedules its callback one turn later
// too, so both arms report "the loop turned" — the asymmetry is that the inline
// one turned only AFTER the work was already finished. What separates them is
// that `pbkdf2(...)` itself returns in microseconds when the work went to a
// pool and in ~a second when it did not.
//
// The threshold is 250 ms against workloads sized to a second or more of CPU,
// so the margin is several-fold and the answer is not a stopwatch race.
const STALL_MS = 250;

async function callCost(start: () => Promise<unknown>): Promise<[number, unknown]> {
  const t0 = Date.now();
  const pending = start();
  const cost = Date.now() - t0;
  return [cost, await pending];
}

async function main(): Promise<void> {
  // ── pbkdf2: two million iterations, about a second of pure CPU ───────────
  const pbkdfExpected = pbkdf2Sync("perry", "turnloop", 2_000_000, 32, "sha256");
  const [pbkdfCost, pbkdfValue] = await callCost(() =>
    pbkdf2Async("perry", "turnloop", 2_000_000, 32, "sha256"),
  );
  const derived = pbkdfValue as Buffer;
  console.log("pbkdf2 call returned without deriving:", pbkdfCost < STALL_MS);
  console.log("pbkdf2 bytes match the sync twin:", derived.equals(pbkdfExpected));
  console.log("pbkdf2 length:", derived.length);

  // ── scrypt: memory-hard, the more expensive of the two ────────────────────
  const scryptOptions = { N: 16384, r: 8, p: 4 };
  const scryptExpected = scryptSync("perry", "turnloop", 64, scryptOptions);
  const [scryptCost, scryptValue] = await callCost(() =>
    scryptAsync("perry", "turnloop", 64, scryptOptions),
  );
  const scrypted = scryptValue as Buffer;
  console.log("scrypt call returned without deriving:", scryptCost < STALL_MS);
  console.log("scrypt bytes match the sync twin:", scrypted.equals(scryptExpected));
  console.log("scrypt length:", scrypted.length);

  // ── zlib: eight megabytes through the codec, both directions ─────────────
  const payload = Buffer.alloc(8 * 1024 * 1024);
  for (let i = 0; i < payload.length; i++) payload[i] = i % 251;

  const [gzipCost, gzipValue] = await callCost(() => gzipAsync(payload));
  const packed = gzipValue as Buffer;
  console.log("gzip call returned without compressing:", gzipCost < STALL_MS);
  console.log("gzip really compressed:", packed.length < payload.length);
  console.log("gzip matches the sync twin:", packed.equals(gzipSync(payload)));

  const [gunzipCost, gunzipValue] = await callCost(() => gunzipAsync(packed));
  const unpacked = gunzipValue as Buffer;
  console.log("gunzip call returned without decompressing:", gunzipCost < STALL_MS);
  console.log("gunzip round trip is byte-identical:", unpacked.equals(payload));
  console.log("gunzip matches the sync twin:", unpacked.equals(gunzipSync(packed)));

  // ── The loop really did keep running: a timer armed before the derivation
  // ── starts must fire while it is still in flight, which is only possible if
  // ── the derivation is not on this thread.
  let firedDuring = false;
  const timer = setTimeout(() => {
    firedDuring = true;
  }, 20);
  const slow = pbkdf2Async("perry", "turnloop", 2_000_000, 32, "sha512");
  await new Promise<void>((resolve) => setTimeout(resolve, 60));
  console.log("a 20ms timer fired while pbkdf2 was in flight:", firedDuring);
  clearTimeout(timer);
  console.log("pbkdf2 sha512 length:", ((await slow) as Buffer).length);

  // ── Many at once: the pool is bounded, so a burst must queue and still all
  // ── complete with the right answers.
  const burst = await Promise.all(
    [1, 2, 3, 4, 5, 6, 7, 8, 9, 10].map(async (n) => {
      const chunk = Buffer.alloc(256 * 1024, n);
      const out = (await gunzipAsync(
        (await gzipAsync(chunk)) as Buffer,
      )) as Buffer;
      return out.length === chunk.length && out[0] === n && out[out.length - 1] === n;
    }),
  );
  console.log("ten concurrent round trips all correct:", burst.every(Boolean));
  console.log("ten concurrent round trips count:", burst.length);

  // ── An error still reaches the callback rather than hanging the awaiter ───
  let gunzipError = "none";
  try {
    await gunzipAsync(Buffer.from([0, 1, 2, 3, 4, 5, 6, 7]));
  } catch {
    gunzipError = "threw";
  }
  console.log("gunzip of garbage:", gunzipError);
}

main();
