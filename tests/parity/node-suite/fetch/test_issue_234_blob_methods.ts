// Regression for issue #234 — `await response.blob()` was returning a
// metadata-only `{ size, type }` stub and dropping body bytes. Verify the
// real Blob surface: `.size`, `.type`, `.arrayBuffer()`, `.bytes()`,
// `.text()`, `.slice()` round-trip against Node.

async function main(): Promise<void> {
  // ── 1. Basic blob from a Response, size ──
  const r1 = new Response("hello world");
  const b1 = await r1.blob();
  console.log("size: " + b1.size);

  // ── 2. blob.text() — UTF-8 decode ──
  const r2 = new Response("hello world");
  const b2 = await r2.blob();
  const t2 = await b2.text();
  console.log("text: " + t2);
  console.log("text length: " + t2.length);

  // ── 3. blob.arrayBuffer() — bytes survive ──
  const r3 = new Response("hello");
  const b3 = await r3.blob();
  const ab3 = await b3.arrayBuffer();
  console.log("ab byteLength: " + ab3.byteLength);
  const u8 = new Uint8Array(ab3);
  console.log("u8 length: " + u8.length);
  console.log("u8 bytes: " + u8[0] + "," + u8[1] + "," + u8[2] + "," + u8[3] + "," + u8[4]);

  // ── 4. blob.bytes() — alias ──
  const r4 = new Response("abc");
  const b4 = await r4.blob();
  const ab4 = await b4.bytes();
  const u4 = new Uint8Array(ab4);
  console.log("bytes: " + u4[0] + "," + u4[1] + "," + u4[2]);

  // ── 5. Buffer.from(blob.arrayBuffer()) round-trip ──
  const r5 = new Response("café 🎉");
  const b5 = await r5.blob();
  console.log("multi-byte size: " + b5.size);
  const ab5 = await b5.arrayBuffer();
  const bf = Buffer.from(ab5);
  console.log("multi-byte text: " + bf.toString("utf8"));

  // ── 6. blob.slice(start, end) ──
  const r6 = new Response("abcdefghij");
  const b6 = await r6.blob();
  const s1 = b6.slice(2, 5);
  console.log("slice 2-5 size: " + s1.size);
  const t1 = await s1.text();
  console.log("slice 2-5 text: " + t1);

  // ── 7. blob.slice() with no args (full clone) ──
  const r7 = new Response("xyz");
  const b7 = await r7.blob();
  const s7 = b7.slice();
  const t7 = await s7.text();
  console.log("slice all: " + t7);

  // ── 8. blob.slice(start) — defaults end to length ──
  const r8 = new Response("abcdef");
  const b8 = await r8.blob();
  const s8 = b8.slice(3);
  const t8 = await s8.text();
  console.log("slice from 3: " + t8);

  // ── 9. blob.slice(start, end, type) — with type override ──
  const r9 = new Response("hello");
  const b9 = await r9.blob();
  const s9 = b9.slice(0, 3, "text/plain");
  console.log("typed slice size: " + s9.size);
  console.log("typed slice type: " + s9.type);

  // ── 10. Empty body ──
  const r10 = new Response("");
  const b10 = await r10.blob();
  console.log("empty size: " + b10.size);
  const t10 = await b10.text();
  console.log("empty text length: " + t10.length);

  // ── 11. Negative slice indices count from end ──
  const r11 = new Response("abcdef");
  const b11 = await r11.blob();
  const s11 = b11.slice(-3);
  const t11 = await s11.text();
  console.log("slice -3: " + t11);
}

main();
