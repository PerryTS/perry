// A loop whose trip count the facts bound (a counter with a proven interval
// that every iteration moves one way) and whose emitted body makes no call
// that can collect takes no back-edge poll (`stmt::loops::bounded_poll`,
// `PERRY_FACTS_BOUNDED_LOOPS=0` keeps the polls). Every function below holds
// one loop shape; `PERRY_LOOP_POLL_DIAG=1` prints its verdict at compile time.
// The output is the same with and without the polls: what differs is where a
// pending collection may run, which the allocating controls exercise.

class Grid {
  board: number[] = new Array(128).fill(0);
  log: number[] = [];
  pairs: number[] = [];
  hits = 0;

  constructor() {
    for (let sq = 0; sq < 128; sq++) {
      if ((sq & 0x88) !== 0) continue;
      this.board[sq] = ((sq * 7) % 13) - 6;
    }
  }

  at(sq: number): number {
    return this.board[sq] ?? 0;
  }

  // Bounded, call-free: the two counted loops read the board in its floor.
  nest(): number {
    let s = 0;
    for (let r = 0; r < 8; r++) {
      for (let f = 0; f < 8; f++) {
        if (this.at(r * 16 + f) > 0) s += r * 8 + f;
      }
    }
    return s;
  }

  // The step is one of two literals of opposite sign, chosen before the
  // loop by a comparison of a Number (as `ranks` below by a boolean).
  files(sq: number, dir: number): number {
    let n = 0;
    for (let f = (sq & 7) + (dir > 0 ? 1 : -1); f >= 0 && f < 8; f += dir > 0 ? 1 : -1) {
      if (this.at((sq & 0x70) + f) < 0) n++;
    }
    return n;
  }

  ranks(sq: number, up: boolean): number {
    let n = 0;
    for (let r = (sq >> 4) + (up ? 1 : -1); r >= 0 && r < 8; r += up ? 1 : -1) {
      for (let f = Math.max(0, (sq & 7) - 1); f <= Math.min(7, (sq & 7) + 1); f++) {
        if (this.at(r * 16 + f) === (up ? -1 : 1)) n++;
      }
    }
    return n;
  }

  // The body moves the counter as well, the same way.
  skip(): number {
    let s = 0;
    for (let sq = 0; sq < 128; sq++) {
      if ((sq & 0x88) !== 0) {
        sq += 7;
        continue;
      }
      s += this.at(sq) * (sq & 7);
    }
    return s;
  }

  // A `while` whose last statement moves the counter down.
  down(): number {
    let s = 0;
    let sq = 119;
    while (sq >= 0) {
      if ((sq & 0x88) === 0) s += this.at(sq);
      sq -= 1;
    }
    return s;
  }

  // Control: a call that allocates (the log grows).
  note(v: number): number {
    this.log.push(v);
    return this.log.length;
  }
  withCall(): number {
    let s = 0;
    for (let f = 0; f < 64; f++) {
      if (this.at(f & 7) !== 0) s += this.note(f);
    }
    return s;
  }

  // Control: an allocation in the body (an Array that outlives the
  // iteration; a literal that never leaves the loop is no allocation).
  withAlloc(): number {
    let s = 0;
    for (let f = 0; f < 64; f++) {
      this.pairs = [this.at(f & 7), f, s];
      s += this.pairs.length;
    }
    return s + (this.pairs[1] ?? 0);
  }

  // Control: no counter (the trip count depends on the data).
  collatz(seed: number): number {
    let x = seed;
    let steps = 0;
    while (x !== 1) {
      x = x % 2 === 0 ? x / 2 : 3 * x + 1;
      steps += this.at(x & 0x77) !== 0 ? 1 : 2;
    }
    return steps;
  }

  // Control: a counter the body also resets (it moves both ways).
  reset(): number {
    let s = 0;
    let rounds = 0;
    for (let f = 0; f < 8; f++) {
      s += this.at(f);
      if (f === 7 && rounds < 500) {
        rounds++;
        f = 0;
      }
    }
    return s + rounds;
  }

  // Control: a trip bound past the budget keeps the poll.
  long(): number {
    let s = 0;
    for (let i = 0; i < 300000; i++) s += this.at(i & 0x77);
    return s;
  }

  // Control: the test bounds the counter by another local, so the interval
  // alone is the bound.
  open(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) s += this.at(i & 0x77);
    return s;
  }

  // Three bounded, call-free loops: 7 x 8 back edges per round, `rounds` of
  // them. 1000 rounds are within the budget of 65 536 (`within`); 1100 are
  // past it, and the outer loop keeps its poll (`wide`, a control).
  within(): number {
    let s = 0;
    for (let i = 0; i < 1000; i++) {
      for (let r = (i & 3) + 1; r >= 0 && r < 8; r += 1) {
        for (let f = Math.max(0, (i & 7) - 1); f <= Math.min(7, (i & 7) + 1); f++) {
          if (this.at(r * 16 + f) === 1) s++;
        }
      }
    }
    return s;
  }
  wide(): number {
    let s = 0;
    for (let i = 0; i < 1100; i++) {
      for (let r = (i & 3) + 1; r >= 0 && r < 8; r += 1) {
        for (let f = Math.max(0, (i & 7) - 1); f <= Math.min(7, (i & 7) + 1); f++) {
          if (this.at(r * 16 + f) === 1) s++;
        }
      }
    }
    return s;
  }

  // Control: the collector must still run in the enclosing loop. The outer
  // loop allocates on every iteration (it keeps its poll); the poll-free nest
  // runs between two allocations.
  churn(rounds: number): number {
    let s = 0;
    let keep: number[][] = [];
    for (let i = 0; i < rounds; i++) {
      const row = [i, i + 1, i + 2, this.nest()];
      if ((i & 1023) === 0) keep = [];
      keep.push(row);
      s = (s + (row[3] ?? 0) + keep.length) % 1000003;
    }
    return s + keep.length;
  }
}

const RUN = Math.random() < 2 ? 1 : 0;
const g = new Grid();
console.log("nest", g.nest());
let ranks = 0;
for (let sq = 0; sq < 120 * RUN; sq++) {
  if ((sq & 0x88) !== 0) continue;
  ranks += g.ranks(sq, (sq & 1) === 0) * (sq + 1);
}
console.log("ranks", ranks);
let files = 0;
for (let sq = 0; sq < 120 * RUN; sq++) {
  if ((sq & 0x88) !== 0) continue;
  files += g.files(sq, (sq & 2) === 0 ? 1 : -1) * (sq + 1);
}
console.log("files", files);
console.log("skip", g.skip());
console.log("down", g.down());
console.log("withCall", g.withCall(), g.log.length);
console.log("withAlloc", g.withAlloc());
console.log("collatz", g.collatz(27 * RUN), g.collatz(97));
console.log("reset", g.reset());
console.log("long", g.long());
console.log("open", g.open(90000 * RUN));
console.log("within", g.within());
console.log("wide", g.wide());
console.log("churn", g.churn(60000 * RUN));
