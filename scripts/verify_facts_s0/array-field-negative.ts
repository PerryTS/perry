// `Bag.items` holds an Array or `null`. Under PERRY_FACTS_SABOTAGE=arrfield
// the facts leave the `null` out of the field's representation, so a read
// of the field is claimed to be a plain Array. The witness at the field
// read must report the false proof at the `null` word.
class Bag {
  items: number[] | null = null;
  fill(): void {
    this.items = [1, 2, 3, 4];
  }
  sum(): number {
    const a = this.items;
    if (a === null) return -1;
    let s = 0;
    for (let i = 0; i < 4; i++) s += a[i] ?? 0;
    return s;
  }
}
const full = new Bag();
const empty = new Bag();
if (process.argv.length < 100) full.fill();
console.log(full.sum(), empty.sum());
