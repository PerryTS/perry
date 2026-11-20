// An int32 element read under a bitwise operator is truncated with no test
// (`expr::int32_operand`). The table holds a fraction, so its reads are no
// int32 reads. Under PERRY_FACTS_SABOTAGE=int32read the facts claim every
// Number element read an int32 read: the operand witness must report the
// fraction.
// A loop bound the compiler cannot fold keeps the loop as written.
const RUN = Math.random() < 2 ? 1 : 0;
class Mixer {
  t: number[] = [];
  h = 0;
  constructor() {
    for (let i = 0; i < 16; i++) this.t.push(i * 3);
    this.t.push(1.5);
  }
  mix(k: number): void {
    this.h ^= this.t[k] ?? 0;
  }
}
const m = new Mixer();
for (let i = 0; i < 20 * RUN; i++) m.mix(i);
console.log(m.h);
