const n = Number(process.argv[2] || "100000");
const res: number[] = [];
for (let i = 0; i < n; i++) {
  res.push(i * -2);
}
const start = performance.now();
res.sort((a, b) => a - b);
const sortMs = performance.now() - start;
let checksum = 0;
for (let i = 0; i < n; i++) {
  if (res[i] !== (n - 1 - i) * -2) {
    throw new Error("Incorrect sort at index " + i);
  }
  checksum += res[i];
}
console.log(JSON.stringify({n, sortMs, length: res.length, first: res[0], last: res[n-1], checksum}));
