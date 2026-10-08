// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=11828 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1
// #11828: `Readable.from(asyncGen).pipe(createGunzip())` read with `for await`
// keeps its stream and chunks valid when collections move them between the
// source, the `data` listeners and the pipe writes.
import { Readable } from "node:stream";
import { createGunzip, gzipSync } from "node:zlib";

const sink: string[] = [];
function churn(): void {
  let s = "";
  for (let i = 0; i < 20; i++) s += `q${i}`;
  sink.push(s);
  if (sink.length > 60) sink.length = 0;
}

async function* pieces(gz: Uint8Array, step: number): AsyncGenerator<Uint8Array> {
  for (let at = 0; at < gz.length; at += step) {
    await null;
    churn();
    yield gz.subarray(at, Math.min(gz.length, at + step));
  }
}

function inflate(source: AsyncIterable<Uint8Array>): AsyncIterable<Uint8Array> {
  const input = Readable.from(source);
  const gunzip = createGunzip({ chunkSize: 16384 } as object);
  input.on("error", (e: Error) => gunzip.destroy(e));
  return input.pipe(gunzip);
}

for (let round = 0; round < 3; round++) {
  // Built and sampled without a per-byte JS loop: at a collection per loop
  // poll, a per-byte loop would spend the run's time budget outside the
  // stream windows this test exercises.
  const raw = Uint8Array.from({ length: 20000 + round * 3001 }, (_, i) => (i * 31 + round + (i >> 7)) & 255);
  let total = 0;
  let acc = 0;
  let off = 0;
  for await (const chunk of inflate(pieces(gzipSync(raw), 300 + round * 257))) {
    churn();
    for (let i = (97 - (off % 97)) % 97; i < chunk.length; i += 97) acc = (acc * 33 + chunk[i]) >>> 0;
    off += chunk.length;
    total += chunk.length;
  }
  console.log(round, total, acc);
}
