// A specialized callee hoists the typed-array data pointer. Its caller must
// retain the array even when the call is the binding's last source-level use.
// Also run with PERRY_GC_HEAP_LIMIT=8 PERRY_GEN_GC=0 to force full collections.
let sink: any[] = [];
function sumDuringChurn(buf: Int32Array, count: number): number {
  let sum = 0;
  for (let i = 0; i < count; i++) {
    sink.push({ i, text: "churn-" + i, pair: [i, i + 1] });
    if (sink.length > 4096) sink = [];
    sum = (sum + buf[i & 255]) | 0;
  }
  return sum;
}
function localOwner(): number {
  const local = new Int32Array(256);
  for (let i = 0; i < 256; i++) local[i] = i;
  return sumDuringChurn(local, 320000);
}
const top = new Int32Array(256);
for (let i = 0; i < 256; i++) top[i] = i;
console.log("module-last-use", sumDuringChurn(top, 320000));
console.log("local-last-use", localOwner());
