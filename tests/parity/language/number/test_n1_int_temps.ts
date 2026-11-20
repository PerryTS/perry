// N1 phase 1: integer temporaries and compared-only integer locals keep the
// I32 representation (conditionals, Math.min/max/abs, strict equality and
// switch against int constants), with negative controls that must stay
// double: -0, NaN, 2^31 overflow, `|0` wrap, `>>>` of negatives, `*` near
// 2^53, a counter feeding only double math, and mixed joins.

const board: number[] = new Array<number>(128).fill(0);
for (let i = 0; i < 128; i++) board[i] = (i * 7) % 13 - 6;

function rays(square: number, by: number): number {
  let hits = 0;
  for (const delta of [-17, -16, -15, -1, 1, 15, 16, 17]) {
    let source = square + delta;
    let distance = 1;
    while (source >= 0 && source < 128 && (source & 0x88) === 0) {
      const piece = board[source] ?? 0;
      if (piece !== 0) {
        if (piece * by > 0 && (distance === 1 || Math.abs(piece) === 5)) hits++;
        break;
      }
      source += delta;
      distance++;
    }
  }
  return hits;
}

function passedPawns(): number {
  let total = 0;
  for (let square = 0; square < 128; square++) {
    if ((square & 0x88) !== 0) { square += 7; continue; }
    const p = board[square] ?? 0;
    if (p === 0) continue;
    const file = square & 7;
    const rank = p > 0 ? square >> 4 : 7 - (square >> 4);
    for (let r = (square >> 4) + (p > 0 ? 1 : -1); r >= 0 && r < 8; r += p > 0 ? 1 : -1) {
      for (let f = Math.max(0, file - 1); f <= Math.min(7, file + 1); f++) {
        if ((board[r * 16 + f] ?? 0) === (p > 0 ? -1 : 1)) total += rank === 0 ? 3 : 1;
      }
    }
    const capture = (p & 2) !== 0 ? square - (p > 0 ? 16 : -16) : square;
    switch (Math.abs(p)) {
      case 1: total += capture & 7; break;
      case 4: total -= 2; break;
      default: total += 1;
    }
  }
  return total;
}

// Negative controls.
function controls(c: number): string {
  const out: string[] = [];
  const z = c > 0 ? -0 : 0; // -0 is no int32: stays double
  out.push(String(1 / z));
  const nan = Math.min(c, NaN);
  out.push(String(nan === nan));
  const big = c > 0 ? 2147483647 : 1;
  const over = big + 1; // 2^31: leaves i32
  out.push(String(over), String(over > 2147483647));
  out.push(String((big + c) | 0)); // wraps by ToInt32
  const neg = -5 - c;
  out.push(String(neg >>> 0), String((neg >>> 0) > 5));
  const a = c > 0 ? 94906267 : 3;
  out.push(String((a * a) | 0), String(a * a));
  const m = c > 0 ? 1 : 0.5; // a real merge of an integer and a non-integer
  out.push(String(m + 0.25));
  out.push(String(Math.abs(c > 0 ? -2147483648 : 4)));
  out.push(String(Math.max(c, 2) > 1), String(Math.min(c, -3) === -3));
  let acc = 0.5;
  for (let j = 0; j < 20; j++) acc = acc * 1.01 + j * 0.25; // double-only counter
  out.push(acc.toFixed(6));
  return out.join(" ");
}

// Real GC pressure between the integer loops (PERRY_GC_SCHEDULE_SEED arms).
let keep: number[][] = [];
let sum = 0;
for (let round = 0; round < 300; round++) {
  for (let s = 0; s < 128; s += 3) sum += rays(s, round % 2 === 0 ? 1 : -1);
  sum += passedPawns();
  keep.push(new Array<number>(64).fill(round));
  if (keep.length > 40) keep = [];
}
console.log(sum, keep.length);
console.log(controls(1));
console.log(controls(-1));
