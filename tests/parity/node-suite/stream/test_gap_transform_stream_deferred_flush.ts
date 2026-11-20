// #5989: a TransformStream whose flush() completes asynchronously must have its
// deferred enqueue delivered before the readable side closes.
//
// Next.js' `createBufferedTransformStream` (the first transform in React's
// Fizz → HTTP-response pipeline) buffers every chunk in transform() and emits
// the coalesced chunk from a `setImmediate`; its flush() returns a promise that
// resolves only once that deferred enqueue has run. Per the WHATWG spec the
// transformer's flush completion is awaited before TransformStreamDefaultSink
// closes the readable. Perry closed the readable synchronously right after
// invoking flush(), so the not-yet-enqueued buffered chunk was dropped — every
// force-dynamic Next.js route streamed a 200 with an empty body (content-length
// 0). This test reproduces that exact buffered-transform shape.

function createBufferedTransform() {
  let chunks: Uint8Array[] = [];
  let byteLen = 0;
  let pending: { promise: Promise<void>; resolve: () => void } | undefined;

  const scheduleFlush = (ctrl: TransformStreamDefaultController<Uint8Array>) => {
    if (pending) return;
    let resolve!: () => void;
    const promise = new Promise<void>((r) => (resolve = r));
    pending = { promise, resolve };
    setImmediate(() => {
      try {
        const out = new Uint8Array(byteLen);
        let off = 0;
        for (const c of chunks) {
          out.set(c, off);
          off += c.byteLength;
        }
        chunks = [];
        byteLen = 0;
        ctrl.enqueue(out);
      } finally {
        const p = pending!;
        pending = undefined;
        p.resolve();
      }
    });
  };

  return new TransformStream<Uint8Array, Uint8Array>({
    transform(chunk, ctrl) {
      chunks.push(chunk);
      byteLen += chunk.byteLength;
      scheduleFlush(ctrl);
    },
    flush() {
      if (pending) return pending.promise;
    },
  });
}

async function run() {
  const enc = new TextEncoder();
  const src = new ReadableStream<Uint8Array>({
    start(c) {
      c.enqueue(enc.encode("<html>"));
      c.enqueue(enc.encode("<body>hi</body>"));
      c.enqueue(enc.encode("</html>"));
      c.close();
    },
  });

  const out: number[] = [];
  let total = 0;
  await src.pipeThrough(createBufferedTransform()).pipeTo(
    new WritableStream<Uint8Array>({
      write(chunk) {
        out.push(chunk.byteLength);
        total += chunk.byteLength;
      },
    }),
  );

  console.log("chunks:", JSON.stringify(out));
  console.log("total bytes:", total);
}

// Two chained buffered transforms — the deferred flush of the first must land
// before it closes so the second still receives the payload.
async function chained() {
  const enc = new TextEncoder();
  const dec = new TextDecoder();
  const src = new ReadableStream<Uint8Array>({
    start(c) {
      c.enqueue(enc.encode("alpha"));
      c.enqueue(enc.encode("beta"));
      c.close();
    },
  });
  let seen = "";
  await src
    .pipeThrough(createBufferedTransform())
    .pipeThrough(createBufferedTransform())
    .pipeTo(
      new WritableStream<Uint8Array>({
        write(chunk) {
          seen += dec.decode(chunk, { stream: true });
        },
      }),
    );
  console.log("chained:", seen);
}

run().then(chained);
