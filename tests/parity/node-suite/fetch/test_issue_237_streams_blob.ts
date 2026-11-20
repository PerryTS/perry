// Regression for issue #237 — Web Streams API: ReadableStream from
// blob.stream() / response.body. Acceptance criteria #1 from the issue
// body, plus the immediate followup (`for await`).

async function main(): Promise<void> {
  // ── 1. blob.stream().getReader().read() round-trip ──
  const r1 = new Response("hello world");
  const blob1 = await r1.blob();
  const stream1 = blob1.stream();
  const reader1 = stream1.getReader();
  const first = await reader1.read();
  console.log("first done: " + first.done);
  // value is a Uint8Array — coerce to string by length (the buffered
  // bytes are reachable via .length)
  console.log("first len: " + first.value.length);
  const second = await reader1.read();
  console.log("second done: " + second.done);

  // ── 2. response.body — same shape ──
  const r2 = new Response("abc");
  const body = r2.body;
  const reader2 = body.getReader();
  const out1 = await reader2.read();
  console.log("body chunk done: " + out1.done);
  console.log("body chunk len: " + out1.value.length);
  const out2 = await reader2.read();
  console.log("body second done: " + out2.done);

  // ── 3. for await of stream — desugared to getReader/read loop ──
  const r3 = new Response("xyzzy");
  const blob3 = await r3.blob();
  const stream3 = blob3.stream();
  let totalLen = 0;
  for await (const chunk of stream3) {
    totalLen += chunk.length;
  }
  console.log("for-await total: " + totalLen);
}

main();
