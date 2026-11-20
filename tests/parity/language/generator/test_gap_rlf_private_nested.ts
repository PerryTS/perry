// RLF private frames: a generator driving another generator's frame.
function* range(a: number, b: number): Generator<number> {
  for (let i = a; i < b; i++) yield i;
}

function* chained(): Generator<number> {
  for (const v of range(0, 3)) {
    for (const w of range(0, 2)) yield v * 10 + w;
  }
}
function* churn(n: number): Generator<number> {
  let acc = 0;
  for (let i = 0; i < n; i++) {
    acc += i * 2;
    yield acc + i;
  }
}
let line = "";
for (const v of chained()) line += v + ",";
console.log("chained " + line);
let s = 0;
let junk = "";
for (let r = 0; r < 400; r++) {
  for (const v of churn(200)) {
    junk = "garbage " + v + " " + r;
    s = (s + v) | 0;
  }
}
console.log("churn " + s + " " + junk);
