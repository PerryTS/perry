// Integer intervals (wpfacts `model::range`) prove some bindings int32; the
// lowering then keeps them in an i32 slot. Every binding here must print
// exactly what Node prints, proven or not.

// Loop counters used as indices, one with a body write that skips ahead.
function counters(): string {
  const board: number[] = new Array<number>(128).fill(0);
  for (let sq = 0; sq < 128; sq++) {
    if ((sq & 0x88) !== 0) { sq += 7; continue; }
    board[sq] = (sq >> 4) * 8 + (sq & 7);
  }
  let sum = 0;
  for (let r = 0; r < 8; r++) {
    for (let f = 0; f < 8; f++) sum += board[r * 16 + f] ?? 0;
  }
  return sum + ":" + board[119];
}

// One past the int32 range: never an int32, so never in an i32 slot.
function past(): string {
  let out = "";
  for (let i = 0; i < 3; i++) {
    const k = i + 2147483646;
    out += k + "," + (k | 0) + ";";
  }
  return out;
}

// -0 is not an int32: `0 * -1` and `-x` keep their double.
function negativeZero(): string {
  let out = "";
  for (let i = 0; i < 3; i++) {
    const z = (i - i) * -1;
    const n = -(i - 1);
    out += (1 / z) + " " + (1 / n) + ";";
  }
  return out;
}

// `%` of a negative dividend can be -0 too.
function remainder(): string {
  let out = "";
  for (let i = -4; i < 3; i++) {
    const m = i % 2;
    out += Object.is(m, -0) ? "-0," : m + ",";
  }
  return out;
}

// Math results and `??` over elements whose interval grows with the loop.
function mathAndCoalesce(): number {
  const vals: number[] = [];
  for (let i = 0; i < 40; i++) vals.push((i & 15) - 7);
  let total = 0;
  for (let j = 0; j < 40; j++) {
    const v = vals[j] ?? 0;
    const a = Math.abs(v);
    const lo = Math.min(3, v);
    const hi = Math.max(-2, v);
    const s = Math.sign(v);
    total += v + a * 2 + lo + hi * 3 + s;
  }
  return total;
}

// A parameter fed by several calls covers each of them.
function param(p: number): number {
  let acc = 0;
  for (let i = 0; i < p; i++) acc += i & 3;
  return acc + p;
}

console.log(counters());
console.log(past());
console.log(negativeZero());
console.log(remainder());
console.log(mathAndCoalesce());
console.log(param(3), param(40), param(-2));
