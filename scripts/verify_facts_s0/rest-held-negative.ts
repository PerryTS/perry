// S0 `restheld` (I32 at rest B2): a loop holds the base of an at-rest field
// Array (PR-D) and its served reads take the slot's I32 word as a double.
// The field read call's witness (the real call beside the read) must report
// the value.
class Cells {
  private cells: number[] = new Array<number>(32).fill(0);
  at(i: number): number {
    return this.cells[i] ?? 0;
  }
  put(i: number, v: number): void {
    this.cells[i & 31] = v;
  }
  sum(): number {
    let s = 0;
    for (let i = 0; i < 32; i++) s = (s + this.at(i)) | 0;
    return s;
  }
}
const c = new Cells();
for (let i = 0; i < 32; i++) c.put(i, (i * 5) & 63);
console.log(c.sum(), c.at(3));
