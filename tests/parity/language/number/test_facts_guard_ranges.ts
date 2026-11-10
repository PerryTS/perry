// Integer intervals narrowed by the code a test guards (wpfacts
// `gen::narrow`): branches, `while` bodies, `&&`/`||`, `?:`, the code after
// an early exit, predicate calls, and one-character strings read by
// `Number`. Every value here is checked against what the test allows.
function onBoard(s: number): boolean { return s >= 0 && s < 128 && (s & 0x88) === 0; }

class Board {
  cells: number[] = new Array<number>(128).fill(0);
  at(s: number): number { return this.cells[s] ?? 0; }
  put(s: number, v: number): void { this.cells[s] = v; }
}

function fenRank(rank: string): number[] {
  const out: number[] = [];
  let file = 0;
  for (let i = 0; i < rank.length; i++) {
    const c = rank.charAt(i);
    if (c >= "1" && c <= "8") { file += Number(c); continue; }
    if (file >= 8) throw new Error("rank too long: " + rank);
    out.push(16 + file);
    file++;
  }
  return out;
}

function walk(b: Board, start: number, delta: number): string {
  const seen: number[] = [];
  let s = start;
  while (onBoard(s)) { seen.push(b.at(s)); s += delta; }
  if (!onBoard(start + 1)) return seen.join(",") + "|edge";
  return seen.join(",") + "|" + String(b.at(start + 1));
}

function mixed(x: number, s: string): string {
  const i = x | 0;
  const a = i >= 0 && i < 10 ? i * 2 : -1;
  const n = s.length === 1 && s >= "0" && s <= "9" ? Number(s) : Number("x" + s);
  let k = i;
  if (k > 100) k = k - 1000;
  else if (k < -100) k = k + 1000;
  return [a, n, k, Number(s), Number(s.charAt(0))].join(" ");
}

const b = new Board();
for (let i = 0; i < 128; i++) if ((i & 0x88) === 0) b.put(i, (i * 7) % 13 - 6);
console.log(fenRank("3pk2p1").join(","), fenRank("8").join(","), fenRank("p1p").join(","));
try { console.log(fenRank("pppppppp9").join(",")); } catch (e) { console.log((e as Error).message); }
console.log(walk(b, 0, 17), walk(b, 7, 16), walk(b, 119, -17), walk(b, 3, 1));
console.log(mixed(4, "7"), mixed(40, "a"), mixed(500, ""), mixed(-500, " "), mixed(-3, "12"), mixed(2.5, "9"));
