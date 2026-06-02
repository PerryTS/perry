// Regression for issue #562 — user classes can extend
// WritableStream / ReadableStream / TransformStream and the inherited
// `pipeTo` / `pipeThrough` / underlying-sink callbacks all work.

class MyWritable extends WritableStream<Uint8Array> {
  public seenLengths: number[] = [];
  public closed: boolean = false;
  constructor() {
    super({
      write: (chunk: any): void => {
        this.seenLengths.push(chunk.length);
      },
      close: (): void => {
        this.closed = true;
      },
    });
  }
}

class IdentityTransform extends TransformStream<Uint8Array, Uint8Array> {
  constructor() {
    super({
      transform(chunk: any, controller: any): void {
        controller.enqueue(chunk);
      },
    });
  }
}

class MyReadable extends ReadableStream<Uint8Array> {
  constructor() {
    super({
      start(controller: any): void {
        controller.enqueue(new Uint8Array([1, 2, 3]));
        controller.enqueue(new Uint8Array([4, 5]));
        controller.close();
      },
    });
  }
}

async function main(): Promise<void> {
  // ── 1. pipeTo into a WritableStream subclass ──
  const w = new MyWritable();
  const r1 = new ReadableStream({
    start(c: any): void {
      c.enqueue(new Uint8Array([10, 20, 30]));
      c.enqueue(new Uint8Array([40, 50]));
      c.close();
    },
  });
  await r1.pipeTo(w);
  console.log("subclass-writable lengths: " + w.seenLengths.join(","));
  console.log("subclass-writable closed: " + w.closed);

  // ── 2. pipeThrough a TransformStream subclass ──
  const t = new IdentityTransform();
  const r2 = new ReadableStream({
    start(c: any): void {
      c.enqueue(new Uint8Array([100, 101, 102]));
      c.close();
    },
  });
  const downstream = r2.pipeThrough(t);
  const reader = downstream.getReader();
  const out = await reader.read();
  console.log("subclass-transform done: " + out.done);
  console.log("subclass-transform first len: " + out.value.length);

  // ── 3. ReadableStream subclass producing into a WritableStream subclass ──
  const w2 = new MyWritable();
  const r3 = new MyReadable();
  await r3.pipeTo(w2);
  console.log("subclass-readable->subclass-writable lengths: " + w2.seenLengths.join(","));
  console.log("subclass-readable->subclass-writable closed: " + w2.closed);
}

main();
