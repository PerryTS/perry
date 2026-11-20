// R1 (range tightening): keys whose intervals the facts now prove tighter,
// and the negative controls that must stay wide. Every read goes through
// `at`, a field read call (`this.board[i] ?? -1`) on a board born dense, so
// a key claimed in the floor that is not would read past the elements
// instead of giving -1.

class Board {
  board: number[];
  constructor(n: number) {
    this.board = new Array(n).fill(0);
    for (let i = 0; i < n; i++) this.board[i] = (i * 7) % 13;
  }
  at(i: number): number {
    return this.board[i] ?? -1;
  }

  // (a) sums of two counters: the hull, not the widened sum.
  rankFile(): number {
    let s = 0;
    for (let r = 0; r < 8; r++) for (let f = 0; f < 8; f++) s += this.at(r * 16 + f);
    return s;
  }
  diagonal(): number {
    let s = 0;
    for (let r = 0; r < 8; r++) for (let f = 0; f < 8; f++) s += this.at(r + f);
    return s;
  }
  window(file: number): number {
    let s = 0;
    for (let r = 1; r >= 0 && r < 8; r++) {
      for (let f = Math.max(0, file - 1); f <= Math.min(7, file + 1); f++) s += this.at(r * 16 + f);
    }
    return s;
  }
  // Negative control: the hull is past a 128 floor; the last keys are -1.
  pastHull(): number {
    let s = 0;
    for (let r = 0; r < 8; r++) for (let f = 0; f < 8; f++) s += this.at(r * 16 + f + 9);
    return s;
  }

  // (b) a counter write in a branch that leaves.
  skip(): number {
    let s = 0;
    for (let q = 0; q < 128; q++) {
      if ((q & 0x88) !== 0) { q += 7; continue; }
      s += this.at(q);
    }
    return s;
  }
  skipElse(): number {
    let s = 0;
    for (let q = 0; q < 128; q++) {
      if ((q & 0x88) === 0) { s += 1; } else { q += 7; continue; }
      s += this.at(q);
    }
    return s;
  }
  skipReturn(stop: number): number {
    let s = 0;
    for (let q = 0; q < 128; q++) {
      if (q === stop) { q += 500; return s + this.at(q); }
      s += this.at(q);
    }
    return s;
  }
  skipBreak(stop: number): number {
    let s = 0;
    let q = 0;
    for (q = 0; q < 128; q++) {
      if (q === stop) { q += 500; break; }
      s += this.at(q);
    }
    return s * 1000 + this.at(q);
  }
  guarded(x: number): number {
    if (x < 100) {
      if (x === 42) { x += 1000; return this.at(x); }
      return this.at(x);
    }
    return -7;
  }

  // Negative controls: the write reaches a later read.
  falls(): number {
    let s = 0;
    for (let q = 0; q < 128; q++) {
      if ((q & 0x88) !== 0) { q += 7; }
      s += this.at(q) + 2;
    }
    return s;
  }
  inSwitch(k: number): number {
    let s = 0;
    for (let q = 0; q < 128; q++) {
      switch (k) {
        case 1:
          if (q > 100) { q += 200; break; }
          s += 1;
      }
      s += this.at(q);
    }
    return s;
  }
  inTry(): number {
    let s = 0;
    for (let q = 0; q < 128; q++) {
      try {
        if (q > 100) { q += 200; continue; }
      } finally {
        s += this.at(q);
      }
    }
    return s;
  }
  inCatch(): number {
    let s = 0;
    for (let q = 0; q < 128; q++) {
      try {
        if (q > 100) { q += 200; throw new Error("x"); }
      } catch (e) {
        s += this.at(q) * 3;
      }
    }
    return s;
  }
  labeled(): number {
    let s = 0;
    outer: for (let q = 0; q < 128; q++) {
      if (q > 100) { q += 200; continue outer; }
      s += this.at(q);
    }
    return s;
  }
  innerLoop(): number {
    let s = 0;
    for (let q = 0; q < 128; q++) {
      for (let j = 0; j < 2; j++) {
        if (q > 100 && j === 1) { q += 200; continue; }
        s += this.at(q);
      }
      s += this.at(q);
    }
    return s;
  }
  guardedLoop(x: number): number {
    let s = 0;
    if (x < 100) {
      for (let j = 0; j < 3; j++) {
        if (j === 1) { x += 1000; break; }
      }
      s += this.at(x);
    }
    return s;
  }
}

const b = new Board(128);
console.log(b.rankFile(), b.diagonal(), b.window(0), b.window(3), b.window(7), b.pastHull());
console.log(b.skip(), b.skipElse(), b.skipReturn(40), b.skipReturn(999), b.skipBreak(17), b.skipBreak(999));
console.log(b.guarded(5), b.guarded(42), b.guarded(150));
console.log(b.falls(), b.inSwitch(1), b.inSwitch(2), b.inTry(), b.inCatch(), b.labeled(), b.innerLoop());
console.log(b.guardedLoop(3), b.guardedLoop(120));
const small = new Board(64);
console.log(small.rankFile(), small.diagonal(), small.skip(), small.falls());
