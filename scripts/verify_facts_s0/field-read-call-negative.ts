// `Sub` overrides `at`. Under PERRY_FACTS_SABOTAGE=fieldreadcall the facts
// claim the call is the base method's field read: the field-read-call
// witness (the real call beside the read) must report it.
class Board {
  board: number[] = [1, 2, 3, 4];
  at(i: number): number {
    return this.board[i] ?? 0;
  }
}
class Sub extends Board {
  at(i: number): number {
    return 5;
  }
}
const b: Board = process.argv.length > 0 ? new Sub() : new Board();
let s = 0;
for (let i = 0; i < 4; i++) s += b.at(i & 3);
console.log(s);
