// Test new Request(url, { body }) with a Buffer/Uint8Array body round-trips
// without the +12-byte offset that dropped the first 12 bytes (#5483).
// Expected output:
// string 16: len=16 "ABCDEFGHIJKLMNOP"
// buffer 16: len=16 "ABCDEFGHIJKLMNOP"
// buffer 40: len=40 "0123456789012345678901234567890123456789"
// uint8  6 : len=6 "ABCDEF"
// arraybuffer bytes: [255,0,137,80,78,71]

async function tb(label: string, body: any): Promise<void> {
  const req = new Request("http://h/x", { method: "POST", body });
  const t = await req.text();
  console.log(`${label}: len=${t.length} ${JSON.stringify(t)}`);
}

async function main(): Promise<void> {
  await tb("string 16", "ABCDEFGHIJKLMNOP");
  await tb("buffer 16", Buffer.from("ABCDEFGHIJKLMNOP"));
  await tb("buffer 40", Buffer.from("0123456789012345678901234567890123456789"));
  await tb("uint8  6 ", new Uint8Array([65, 66, 67, 68, 69, 70])); // "ABCDEF"

  // Non-UTF-8 bytes must survive verbatim through arrayBuffer().
  const bin = new Uint8Array([255, 0, 137, 80, 78, 71]);
  const req = new Request("http://h/x", { method: "POST", body: bin });
  const got = new Uint8Array(await req.arrayBuffer());
  console.log("arraybuffer bytes: " + JSON.stringify(Array.from(got)));
}
void main();
