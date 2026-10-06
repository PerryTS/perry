import * as zlib from "zlib";

function sumB(b: Buffer): number {
  let sum = 0;
  for (let i = 0; i < b.length; i++) sum += b[i];
  return sum;
}
function sumU(b: Uint8Array): number {
  let sum = 0;
  for (let i = 0; i < b.length; i++) sum += b[i];
  return sum;
}
function show(name: string, b: Buffer) {
  console.log(name, b.length, b[0], sumB(b), sumU(b), b.toString("hex"));
  const sub = b.subarray(5);
  console.log(name + ".subarray", sub.length, sub[0], sumB(sub), sumU(sub), sub.toString("hex"));
}
const input = Buffer.from("hello hello hello hello hello world");
const gzip = zlib.gzipSync(input);
show("gzip", gzip);
console.log("own roundtrip", zlib.gunzipSync(gzip).toString("hex"));
// Node 26.5.1's exact default gzip, also exercising Node -> Perry gunzip.
const nodeGzip = Buffer.from("1f8b0800000000000003cb48cdc9c957c8c04196e717e5a400009280058923000000", "hex");
console.log("node roundtrip", zlib.gunzipSync(nodeGzip).toString("hex"));
show("empty", zlib.gzipSync(Buffer.alloc(0)));
