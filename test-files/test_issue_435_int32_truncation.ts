// Regression for #435: `is_int32_producing_expr` accepted `Add | Sub |
// Mul` over int-stable operands, so any integer accumulator written
// through these operators was treated as int-stable and got an i32
// shadow slot — silently truncating 64-bit results to 32-bit signed
// integers when the running sum / product / step exceeded i32 range.
//
// JS arithmetic is f64; the i32 closure under `+/−/×` only holds when
// each step's *mathematical* result fits in i32. The recent `>>> 0`
// exclusion (commit 817c4b56) patched one instance of the same class;
// this issue documents 9 more.
//
// Fix: gate the Let-site i32 shadow on a transitive-closure
// `index_used_locals` analysis.  Locals whose value flows into an
// array index (directly or via arithmetic chain) keep the i32 shadow
// — preserving image_convolution's `xx → idx → array[idx]` path that
// motivated dropping the original v0.5.164 gate.  Pure accumulators
// that never reach an index (`sum += compute(i)`) lose the shadow and
// stay on the f64 slot, matching JS semantics.
//
// Each test below is one of the bug-table rows from issue #435.

// #1: canonical sum += compute(i)
function bug1(): void {
  function compute(x: number): number {
    return x * 2 + 1;
  }
  let sum = 0;
  for (let i = 0; i < 50000000; i++) sum = sum + compute(i);
  console.log("bug1:", sum);
}

// #2: const big = a*b both int-stable, product > i32
function bug2(): void {
  const a: number = 100000;
  const b: number = 100000;
  const big = a * b;
  console.log("bug2:", big);
}

// #3: const big = literal * literal
function bug3(): void {
  const big = 100000 * 100000;
  console.log("bug3:", big);
}

// #4: acc -= 100 past -2^31
function bug4(): void {
  let acc = 0;
  for (let i = 0; i < 50000000; i++) acc -= 100;
  console.log("bug4:", acc);
}

// #5: factorial via prod *= i
function bug5(): void {
  let prod = 1;
  for (let i = 1; i <= 15; i++) prod *= i;
  console.log("bug5:", prod);
}

// #6: const big = a+b both int-stable, sum > i32
function bug6(): void {
  const a: number = 2000000000;
  const b: number = 2000000000;
  const big = a + b;
  console.log("bug6:", big);
}

// #7: TS-typed `: number` sum accumulator
function bug7(): void {
  let sum: number = 0;
  for (let i = 0; i < 50000000; i++) sum = sum + i * 2 + 1;
  console.log("bug7:", sum);
}

// #8: const c = b + b via forward-closure pass
function bug8(): void {
  const b = 2000000000;
  const c = b + b;
  console.log("bug8:", c);
}

// #9: i = i + 100 strided counter past i32
function bug9(): void {
  let i = 0;
  for (let n = 0; n < 40000000; n++) i = i + 100;
  console.log("bug9:", i);
}

// Control: pure index use stays on the i32 path (perf invariant).
// `idx = (row + xx) * 3` chained into `arr[idx]` should still produce
// correct results — image_conv's hot inner loop relies on this.
function control(): void {
  const arr = new Array(48);
  for (let i = 0; i < 48; i++) arr[i] = i;
  let acc = 0;
  for (let yy = 0; yy < 4; yy++) {
    for (let xx = 0; xx < 4; xx++) {
      const row = yy * 4;
      const idx = (row + xx) * 3;
      acc = acc + arr[idx];
    }
  }
  console.log("control:", acc);
}

bug1();
bug2();
bug3();
bug4();
bug5();
bug6();
bug7();
bug8();
bug9();
control();
