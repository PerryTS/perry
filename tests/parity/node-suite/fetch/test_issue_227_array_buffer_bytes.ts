// Regression for issue #227 — `await response.arrayBuffer()` was returning a
// metadata-only `{ byteLength: N }` stub, causing `new Uint8Array(buf)` and
// `Buffer.from(buf)` to see 0 bytes. Verify body bytes survive the round trip.
//
// Expected output:
// arrayBuffer byteLength: 5
// uint8array length: 5
// uint8array first byte: 104
// uint8array last byte: 111
// uint8array bytes: 104,101,108,108,111
// buffer length: 5
// buffer first byte: 104
// buffer toString utf8: hello
// empty body byteLength: 0
// empty uint8array length: 0
// non-ascii byteLength: 10
// non-ascii toString: café 🎉
// json roundtrip: 42

async function main(): Promise<void> {
  // ── Text body round-trip ──
  const r1 = new Response("hello");
  const ab1 = await r1.arrayBuffer();
  console.log("arrayBuffer byteLength: " + ab1.byteLength);

  const u8 = new Uint8Array(ab1);
  console.log("uint8array length: " + u8.length);
  console.log("uint8array first byte: " + u8[0]);
  console.log("uint8array last byte: " + u8[4]);
  const parts: number[] = [];
  for (let i = 0; i < u8.length; i++) parts.push(u8[i]);
  console.log("uint8array bytes: " + parts.join(","));

  const r2 = new Response("hello");
  const ab2 = await r2.arrayBuffer();
  const bf = Buffer.from(ab2);
  console.log("buffer length: " + bf.length);
  console.log("buffer first byte: " + bf[0]);
  console.log("buffer toString utf8: " + bf.toString("utf8"));

  // ── Empty body ──
  const r3 = new Response("");
  const ab3 = await r3.arrayBuffer();
  console.log("empty body byteLength: " + ab3.byteLength);
  const u8empty = new Uint8Array(ab3);
  console.log("empty uint8array length: " + u8empty.length);

  // ── Multi-byte UTF-8 body (4-byte emoji + 2-byte é) ──
  const r4 = new Response("café 🎉");
  const ab4 = await r4.arrayBuffer();
  console.log("non-ascii byteLength: " + ab4.byteLength);
  const bf4 = Buffer.from(ab4);
  console.log("non-ascii toString: " + bf4.toString("utf8"));

  // ── JSON body decoded via Buffer.from(arrayBuffer).toString → JSON.parse ──
  const r5 = new Response(JSON.stringify({ value: 42 }));
  const ab5 = await r5.arrayBuffer();
  const text5 = Buffer.from(ab5).toString("utf8");
  const parsed = JSON.parse(text5);
  console.log("json roundtrip: " + parsed.value);
}

main();
