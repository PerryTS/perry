// N12: a body keeps one stack check per path. The checks its inlined helpers
// bring along are dropped when an earlier check of the same body dominates
// them. Every recursion below must still end in a catchable RangeError, and
// the bounded runs must compute what they did before.
class Board {
  cells: number[] = [];
  names: string[] = [];
  constructor() {
    for (let i = 0; i < 64; i++) {
      this.cells.push((i * 7) % 13);
      this.names.push("s" + i);
    }
  }
  at(sq: number): number {
    return this.cells[sq] ?? 0;
  }
  name(sq: number): string {
    return this.names[sq] ?? "?";
  }
  file(sq: number): number {
    return sq & 7;
  }
  rank(sq: number): number {
    return sq >> 3;
  }
  // Several inlined helpers, then the recursion: the helpers' checks sit
  // under the body's own.
  walk(sq: number, depth: number): number {
    const here = this.at(sq) + this.file(sq) + this.rank(sq) + this.name(sq).length;
    if (depth === 0) return here;
    return here + this.walk((sq + this.at(sq) + 1) % 64, depth - 1);
  }
  // The same body without a way out.
  fall(sq: number): number {
    const here = this.at(sq) + this.file(sq) + this.rank(sq) + this.name(sq).length;
    return here + this.fall((sq + 1) % 64);
  }
  // Negative control: the helpers run on one arm only; the recursion sits on
  // the join and is covered by no helper's check.
  oneArm(sq: number, flag: boolean): number {
    let v = 0;
    if (flag) v = this.at(sq) + this.name(sq).length;
    return v + this.oneArm((sq + 1) % 64, !flag);
  }
  // Helpers before a loop, the recursion inside it.
  inLoop(sq: number): number {
    let total = this.at(sq) + this.file(sq);
    for (let i = 0; i < 2; i++) {
      total += this.name((sq + i) % 64).length;
      total += this.inLoop((sq + i + 1) % 64);
    }
    return total;
  }
  // Mutual recursion through a body small enough to inline.
  ping(sq: number): number {
    return this.at(sq) + this.pong((sq + 1) % 64);
  }
  pong(sq: number): number {
    return this.file(sq) + this.ping((sq + 1) % 64);
  }
}

function caught(label: string, run: () => number): void {
  try {
    console.log(label, "returned", run());
  } catch (e: any) {
    console.log(label, e instanceof RangeError, e.message);
  }
}

const board = new Board();
console.log("walk", board.walk(3, 0), board.walk(3, 10), board.walk(60, 500));
caught("fall", () => board.fall(0));
caught("oneArm", () => board.oneArm(0, true));
caught("inLoop", () => board.inLoop(0));
caught("ping", () => board.ping(0));
// Still usable after each overflow, and the checks fire again.
console.log("walk again", board.walk(5, 200));
caught("fall again", () => board.fall(7));

// Negative control: a body whose only call that can run user code is the
// cold arm of a comparison. It is not a leaf: valueOf recurses through it.
function less(a: any, b: any): boolean {
  return a < b;
}
let turns = 0;
const loopy = {
  valueOf(): number {
    turns++;
    return less(loopy, 1) ? 1 : 0;
  },
};
try {
  console.log("less returned", less(loopy, 2));
} catch (e: any) {
  console.log("less", e instanceof RangeError, e.message, turns > 100);
}
console.log("less plain", less(1, 2), less(2, 1), less("a", "b"));

// Recursion through a callback handed to an inlined helper.
function each(xs: number[], f: (x: number) => number): number {
  let s = 0;
  for (const x of xs) s += f(x);
  return s;
}
function fan(n: number): number {
  const xs = [n, n + 1];
  return each(xs, (x) => fan(x + 1)) + xs.length;
}
caught("fan", () => fan(0));

// Allocation between the checks: the dropped overflow arms held roots.
function build(n: number, acc: string[]): string[] {
  const label = "n" + n;
  const pair = [label, label + "!"];
  acc.push(pair[0] + pair[1].length);
  if (n === 0) return acc;
  return build(n - 1, acc);
}
const built = build(300, []);
console.log("build", built.length, built[0], built[299], built[300]);
function buildForever(n: number, acc: string[]): string[] {
  const label = "n" + n;
  const pair = [label, label + "!"];
  if (acc.length < 64) acc.push(pair[0] + pair[1].length);
  return buildForever(n + 1, acc);
}
const kept: string[] = [];
try {
  buildForever(0, kept);
} catch (e: any) {
  console.log("buildForever", e instanceof RangeError, kept.length, kept[0], kept[63]);
}
