// Regression for issue #320 — `new ReadableStream({ start, pull, cancel })` +
// controller.enqueue / controller.close used to fail at link with
// "Undefined symbols: _js_readable_stream_new, _js_readable_stream_controller_*"
// because perry-codegen emitted FFI calls but perry-stdlib provided no
// implementation. Resolved by #237/#301 (Web Streams API). This test pins the
// exact code shape from the issue body — string chunks (not Uint8Array, which
// the #237 tests already cover) — to guard against regressions in either the
// FFI link surface or the string-chunk read path.

async function main(): Promise<void> {
  const stream = new ReadableStream({
    start(controller: any): void {
      controller.enqueue("hello");
      controller.enqueue("world");
      controller.close();
    },
  });

  const reader = stream.getReader();
  const r1 = await reader.read();
  console.log("r1 done: " + r1.done);
  console.log("r1 value: " + r1.value);
  const r2 = await reader.read();
  console.log("r2 done: " + r2.done);
  console.log("r2 value: " + r2.value);
  const r3 = await reader.read();
  console.log("r3 done: " + r3.done);
}

main();
