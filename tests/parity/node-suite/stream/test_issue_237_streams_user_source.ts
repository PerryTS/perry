// Regression for issue #237 — user-supplied ReadableStream source via
// `new ReadableStream({ start, pull, cancel })` + `controller.enqueue` /
// `controller.close()`. Verifies the controller surface and the
// pending-read drain semantics.

async function main(): Promise<void> {
  console.log("enter");
  // ── 1. start enqueues + closes synchronously ──
  const stream = new ReadableStream({
    start(controller: any): void {
      controller.enqueue(new Uint8Array([72, 101, 108, 108, 111]));  // "Hello"
      controller.enqueue(new Uint8Array([32, 87, 111, 114, 108, 100]));  // " World"
      controller.close();
    },
  });

  const reader = stream.getReader();
  const r1 = await reader.read();
  console.log("r1 done: " + r1.done);
  console.log("r1 len: " + r1.value.length);

  const r2 = await reader.read();
  console.log("r2 done: " + r2.done);
  console.log("r2 len: " + r2.value.length);

  const r3 = await reader.read();
  console.log("r3 done: " + r3.done);
  console.log("end");
}

main();
