// DESIGN-p4l1 §5.4 fixture 13: a loop that allocates and reads a field-held
// array 10^5 times, so collections run between the field load and the
// element load of other iterations. Run under PERRY_GC_MOVING_LOOP_POLLS=1
// and a seeded PERRY_GC_STRESS as well.

class Grid {
  cells: number[] = new Array(64).fill(0);
  log: string[] = [];
  at(i: number): number {
    return this.cells[i] ?? 0;
  }
  run(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) {
      const k = i & 63;
      this.cells[k] = (i * 31) % 97;
      const pair = { a: this.at(k), b: this.cells[(k + 1) & 63] ?? 0 };
      if ((i & 1023) === 0) this.log.push("s" + pair.a);
      s = (s + pair.a * 3 + pair.b + this.at((k * 5) & 63)) % 1000003;
    }
    return s;
  }
}
const g = new Grid();
console.log(g.run(100000), g.log.length, g.log[3], g.at(63), g.at(64));
