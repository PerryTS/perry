// Test new Response(Uint8Array/ArrayBuffer) binary body round-trips (#5435)
// Expected output:
// uint8array body bytes: [1,2,3,4,5]
// arraybuffer body bytes: [1,2,3,4,5]
// binary body bytes: [255,0,137,80,78,71]
// string body text: hello

async function main(): Promise<void> {
  const data = new Uint8Array([1, 2, 3, 4, 5]);

  const rb = new Response(data as BodyInit);
  const got = new Uint8Array(await rb.arrayBuffer());
  console.log("uint8array body bytes: " + JSON.stringify(Array.from(got)));

  const ra = new Response(data.buffer as BodyInit);
  const gotA = new Uint8Array(await ra.arrayBuffer());
  console.log("arraybuffer body bytes: " + JSON.stringify(Array.from(gotA)));

  // Non-UTF-8 bytes (PNG header-ish) must survive verbatim, not be dropped.
  const bin = new Uint8Array([255, 0, 137, 80, 78, 71]);
  const rbin = new Response(bin as BodyInit);
  const gotBin = new Uint8Array(await rbin.arrayBuffer());
  console.log("binary body bytes: " + JSON.stringify(Array.from(gotBin)));

  // String bodies must still round-trip unchanged.
  const rs = new Response("hello");
  console.log("string body text: " + (await rs.text()));
}
void main();
