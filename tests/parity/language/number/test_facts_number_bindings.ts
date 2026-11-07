// Bindings the whole-program facts prove hold only Numbers take the raw-double
// lowering: arithmetic, comparison, equality, update and string conversion
// must keep Number semantics for every Number, including -0, NaN, the
// infinities, fractions and values past the i32 range.
const RAYS = [-17, -16, -15, -1, 1, 15, 16, 17];

function onBoard(square: number): boolean {
  return square >= 0 && square < 128 && (square & 0x88) === 0;
}

class Board {
  private cells: number[] = new Array<number>(128).fill(0);
  constructor() {
    for (let i = 0; i < 128; i++) if ((i & 0x88) === 0) this.cells[i] = ((i * 7) % 13) - 6;
  }
  private at(square: number): number { return this.cells[square] ?? 0; }
  rays(square: number, color: number): number {
    let hits = 0;
    for (const delta of RAYS) {
      let source = square + delta;
      let distance = 1;
      while (onBoard(source)) {
        const piece = this.at(source);
        if (piece !== 0) {
          if (piece * color > 0 && (distance === 1 || Math.abs(piece) === 5)) hits += 1;
          break;
        }
        source += delta;
        distance++;
      }
    }
    return hits;
  }
}

function mix(a: number, b: number): number[] {
  const s = a + b;
  const d = a - b;
  const q = a / b;
  const m = a % b;
  let acc = 0;
  acc += s * d;
  acc++;
  return [s, d, q, m, acc, s === d ? 1 : 0, s < d ? 1 : 0, a === b ? 1 : 0, a !== a ? 1 : 0];
}

function same(a: number, b: number): string {
  return [a === b, a !== b, a < b, a >= b, Object.is(a, b), String(a), "" + b].join(",");
}

function late(flag: boolean): number {
  let x = 1;
  if (flag) x = 2.5;
  x *= 2;
  return x;
}

const values = [0, -0, 1, -1, 2.5, -7.25, 2147483647, 2147483648, -2147483649, 1e21, NaN, Infinity, -Infinity];
for (const a of values) {
  for (const b of values) console.log(a, b, mix(a, b).join(" "), same(a, b));
}
const board = new Board();
let total = 0;
for (let sq = 0; sq < 128; sq++) {
  if (!onBoard(sq)) continue;
  total += board.rays(sq, 1) * 3 + board.rays(sq, -1);
}
console.log("rays", total, late(true), late(false));

// Not Numbers only: a binding that also sees a string or undefined keeps the
// generic lowering, so `+` concatenates and a missing argument reads undefined.
function widen(a: number | string, b?: number): string {
  const c = a + 1;
  return String(c) + ":" + String(b) + ":" + String((b ?? 0) + 1);
}
console.log(widen(1, 2), widen("1"), widen(3));
let unset: number | undefined;
if (total < 0) unset = 4;
console.log(String(unset), typeof unset);

// A Number-only accumulator whose writes add a value that is not one:
// `.length` of a Number is undefined, so the sum is NaN, not undefined.
const mixed: any[] = ["ab", 42, "cde"];
function lengths(): number {
  let total = 0;
  for (let i = 0; i < 3; i++) total += mixed[i].length;
  return total;
}
console.log(lengths(), String(lengths()), typeof lengths());
