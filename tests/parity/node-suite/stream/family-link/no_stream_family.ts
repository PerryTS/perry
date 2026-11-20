// Negative control: an ordinary program that names no stream, events, http or
// process stream member. Its binary links no node_stream code (size gate); it
// prints through console.log only.
class P {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
const a: P[] = [];
for (let i = 0; i < 10; i++) a.push(new P(i));
const m = new Map<string, number>();
for (const p of a) m.set("k" + p.x, p.x * 2);
let s = 0;
for (const [, v] of m) s += v;
console.log(JSON.stringify({ s, n: a.length, argv: process.argv.length > 0 }));
