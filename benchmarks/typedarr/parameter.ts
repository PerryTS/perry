function scan(L: Int32Array): number {
  let sum = 0;
  for (let pass = 0; pass < 1000; pass++) {
    for (let i = 0; i < L.length; i++) sum += L[i] < 0 ? -1 : 1;
  }
  return sum;
}
const data = new Int32Array(4096);
for (let i = 0; i < data.length; i++) data[i] = i - 2048;
console.log(scan(data), data[12]);
