// Fixed-size batches expose short-array growth; the large arm amortizes it.
function growing(width: number, batches: number): number {
  let sum = 0;
  for (let batch = 0; batch < batches; batch++) {
    const values: number[] = [];
    for (let i = 0; i < width; i++) values.push(i + batch);
    for (let i = 0; i < values.length; i++) sum += values[i];
  }
  return sum;
}
const width = Number(process.argv[2]);
const batches = Number(process.argv[3]);
console.log(growing(width, batches));
