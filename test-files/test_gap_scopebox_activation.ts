// One activation across callbacks with different capture sets.
function activation(seed: number) {
  let a = seed, b = seed + 1, c = seed + 2;
  const x = () => { a++; c += 2; return a + c; };
  const y = () => { a += 3; b++; return a + b; };
  return [x, y, () => a + b + c];
}
const first = activation(1), second = activation(10);
console.log(first[0](), first[1](), first[2](), second[2](), first[2]());
// Each loop-body entry has fresh lexical storage, shared within that entry.
function iterations() {
  const reads: (() => number)[] = [], writes: (() => void)[] = [];
  for (let i = 0; i < 4; i++) {
    let a = i, b = i + 10;
    reads.push(() => a + b);
    writes.push(() => { a += 100; b++; });
  }
  writes[1](); writes[3]();
  console.log(reads[0](), reads[1](), reads[2](), reads[3]());
}
iterations();
// Loop-head bindings must remain distinct through update and continue.
function heads() {
  const callbacks: (() => number)[] = [];
  for (let i = 0; i < 4; i++) { callbacks.push(() => i); if (i === 1) continue; }
  console.log(callbacks[0](), callbacks[1](), callbacks[2](), callbacks[3]());
}
heads();
function indexed(values: any[]) {
  let items = values, index = 0, sum = 0;
  const step = () => { sum += items[index]; index++; };
  step(); step();
  items = [20, 30, 40]; step();
  console.log(sum, index);
}
indexed([1, 2, 3]);
function multipleHeads() {
  const callbacks: (() => number)[] = [];
  for (let i = 0, j = 10; i < 3; i++, j++) callbacks.push(() => i + j);
  console.log(callbacks[0](), callbacks[1](), callbacks[2]());
}
multipleHeads();
function tdz() {
  try {
    const read = () => late;
    console.log(read());
    let late = 5;
    late++;
  } catch (error) { console.log(error instanceof ReferenceError); }
}
tdz();
