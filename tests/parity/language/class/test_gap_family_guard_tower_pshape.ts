// A class born on a family carries the family descriptor word, not the static
// id of its keys: the dispatch tower and the fact checks must admit that word.
interface Kinged {
  k(): number;
}
class G {
  private side: string = "white";
  private wk = -1;
  constructor(s: string) {
    this.wk = s.length;
  }
  private king(c: string): number {
    return c === "white" ? this.wk : 0;
  }
  k(): number {
    return this.king(this.side);
  }
}
const g = new G("..K..k");
console.log(g.k());
function probe(r: Kinged): number {
  return r.k();
}
let total = 0;
for (let i = 0; i < 3; i++) total += probe(new G("abc".repeat(i + 1)));
console.log(probe(g), total);
