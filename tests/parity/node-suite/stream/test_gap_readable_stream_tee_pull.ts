// `ReadableStream.tee()` must return two branches that both lazily PULL the
// shared source: reading a branch drives the source's `pull`, and each chunk
// fans out to both branches. The previous implementation snapshot-drained the
// source's current buffer and closed it, so a pull-driven source (one that only
// produces inside `pull`, e.g. react-server-dom's RSC flight producer with
// `highWaterMark: 0`) yielded two EMPTY branches — the reader saw `done: true`
// immediately. That silently broke Next.js App Router SSR (the HTML render
// consumes the tee'd RSC flight), which hung forever (#5989).
//
// Validated byte-for-byte against `node --experimental-strip-types`.

async function readAll(r: any): Promise<string> {
  const dec = new TextDecoder();
  let out = "";
  while (true) {
    const { done, value } = await r.read();
    if (done) break;
    out += typeof value === "string" ? value : dec.decode(value as Uint8Array);
  }
  return out;
}

async function main() {
  // (1) pull-driven byte source, highWaterMark 0 — the flight-producer shape.
  let pulls = 0;
  const src = new ReadableStream(
    {
      type: "bytes",
      pull(c: any) {
        pulls++;
        c.enqueue(new TextEncoder().encode(`d${pulls}`));
        if (pulls >= 3) c.close();
      },
    },
    { highWaterMark: 0 },
  );
  const [a, b] = src.tee();
  const outA = await readAll(a.getReader());
  const outB = await readAll(b.getReader());
  console.log(pulls, outA, outB);

  // (2) eagerly-buffered source — both branches see the queued chunks.
  const s2 = new ReadableStream({
    start(c: any) {
      c.enqueue("a");
      c.enqueue("b");
      c.close();
    },
  });
  const [c1, c2] = s2.tee();
  console.log(await readAll(c1.getReader()), await readAll(c2.getReader()));

  // (3) read only ONE branch — it still drives the source to completion.
  let p3 = 0;
  const s3 = new ReadableStream({
    pull(c: any) {
      p3++;
      c.enqueue("x" + p3);
      if (p3 >= 2) c.close();
    },
  });
  const [d1] = s3.tee();
  console.log(await readAll(d1.getReader()), p3);

  // (4) interleaved reads across both branches.
  const s4 = new ReadableStream({
    pull(c: any) {
      c.enqueue("z");
      c.close();
    },
  });
  const [e1, e2] = s4.tee();
  const ra = e1.getReader();
  const rb = e2.getReader();
  const r1 = await ra.read();
  const r2 = await rb.read();
  console.log(r1.value, r2.value, r1.done, r2.done);

  // (5) an error on the source rejects reads on both branches.
  const s5 = new ReadableStream({
    pull(c: any) {
      c.error(new Error("boom"));
    },
  });
  const [f1, f2] = s5.tee();
  let ea = "no";
  let eb = "no";
  try {
    await f1.getReader().read();
  } catch (e: any) {
    ea = e.message;
  }
  try {
    await f2.getReader().read();
  } catch (e: any) {
    eb = e.message;
  }
  console.log(ea, eb);
}

main();
