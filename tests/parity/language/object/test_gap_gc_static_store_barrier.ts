// A store of a binary-image String literal takes no write barrier
// (`expr/write_barrier.rs::with_static_child`); every other store keeps it.
// Old holders receive young heap strings, built at run time, beside literal
// stores and a literal-returning call, under allocation churn heavy enough to
// reach the collector. If the exemption ever admitted a heap string, a minor
// collection would miss the old-to-young edge and the checksum below would
// read a freed or moved string.

type Color = "white" | "black";
function opposite(c: Color): Color { return c === "white" ? "black" : "white"; }

class Holder {
  label: any = "start";
  side: Color = "white";
  flip(): void { this.side = opposite(this.side); }
  plain(): void { this.label = "plain"; }
  heap(i: number): void { this.label = "h" + i + ":" + (i * 7); }
  pick(odd: boolean): void { this.label = odd ? "odd" : "even"; }
}

const holders: Holder[] = [];
for (let i = 0; i < 2000; i++) holders.push(new Holder());

let junk: any[] = [];
let sum = 0;
for (let round = 0; round < 200; round++) {
  for (let i = 0; i < holders.length; i++) {
    const h = holders[i];
    h.flip();
    switch ((i + round) % 3) {
      case 0: h.plain(); break;
      case 1: h.heap(i + round); break;
      default: h.pick((i & 1) === 1);
    }
    junk.push({ a: i, b: [round, i], c: "x" + i });
    if (junk.length > 4000) junk = [];
  }
  for (let i = 0; i < holders.length; i++) {
    const s: string = holders[i].label;
    sum = (sum * 31 + s.length + s.charCodeAt(s.length - 1)) | 0;
    if (holders[i].side !== "white" && holders[i].side !== "black") sum ^= 1;
  }
}
console.log(holders[1].label, holders[2].label, holders[3].side, sum);
