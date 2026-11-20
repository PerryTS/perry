// A loop's trip bound comes from its counter's interval, when every write of
// the counter in the loop moves it the same way (`stmt::loops::bounded_poll`):
// the facts give `f` a small interval (it never leaves [0, 8]), so a loop
// that only ever raises it runs at most as many times as the interval has
// integers. `sweep` also sets `f` back to 0, so it has no such proof, and
// runs 800 iterations. Under PERRY_FACTS_SABOTAGE=looptrip the other writes
// are not looked at and the interval alone is taken as the bound; the
// iteration count at the end of the body must report the first iteration
// past it.
// A loop bound the compiler cannot fold keeps the loop as written.
const RUN = Math.random() < 2 ? 1 : 0;
class Grid {
  board: number[] = new Array(16).fill(3);
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  sweep(): number {
    let s = 0;
    let rounds = 0;
    for (let f = 0; f < 8; f++) {
      s += this.at(f);
      if (f === 7 && rounds < 99 * RUN) {
        rounds++;
        f = 0;
      }
    }
    return s + rounds;
  }
}
console.log(new Grid().sweep());
