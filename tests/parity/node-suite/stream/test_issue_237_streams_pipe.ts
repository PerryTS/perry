// Regression for issue #237 — pipeTo / pipeThrough / WritableStream /
// TransformStream end-to-end.

async function main(): Promise<void> {
  // ── 1. pipeTo: drain readable into writable ──
  const seen: any[] = [];
  const ws = new WritableStream({
    write(chunk: any): void {
      seen.push(chunk.length);
    },
    close(): void {
      seen.push(-1);
    },
  });

  const rs = new ReadableStream({
    start(c: any): void {
      c.enqueue(new Uint8Array([1, 2, 3]));
      c.enqueue(new Uint8Array([4, 5]));
      c.close();
    },
  });

  await rs.pipeTo(ws);
  console.log("pipeTo lengths: " + seen.join(","));

  // ── 2. pipeThrough: identity transform ──
  const ts = new TransformStream({
    transform(chunk: any, controller: any): void {
      controller.enqueue(chunk);
    },
  });

  const upstream = new ReadableStream({
    start(c: any): void {
      c.enqueue(new Uint8Array([10, 20, 30]));
      c.close();
    },
  });

  const downstream = upstream.pipeThrough(ts);
  const reader = downstream.getReader();
  const out = await reader.read();
  console.log("through done: " + out.done);
  console.log("through len: " + out.value.length);

  // ── 3. tee: returns an array of two ReadableStream handles. Each
  // branch holds its own copy of the buffered chunks. The branches[0] /
  // branches[1] indexing pattern doesn't currently retag the elements
  // as ReadableStream native instances; downstream consumers can drive
  // the branches via direct FFI for now (real propagation through
  // index access is tracked as a #237 followup). Verify the array
  // shape here so the codegen path stays exercised.
  const teeable = new ReadableStream({
    start(c: any): void {
      c.enqueue(new Uint8Array([1, 2]));
      c.enqueue(new Uint8Array([3, 4]));
      c.close();
    },
  });
  const branches = teeable.tee();
  console.log("tee length: " + branches.length);
}

main();
