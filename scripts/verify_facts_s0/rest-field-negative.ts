// `Acc.v` holds Numbers past int32 (2**31 + k). Under
// PERRY_FACTS_SABOTAGE=restfield the facts claim the field I32 whatever its
// interval (I32 at rest, PR A). The witness at the proven field read must
// report the false claim at the first word that is no exact int32.
class Acc {
  v = 0;
  bump(k: number): void {
    this.v = 2147483648 + k;
  }
  read(): number {
    return this.v & 255;
  }
}
const a = new Acc();
let s = 0;
for (let i = 0; i < 10; i++) {
  a.bump(i);
  s += a.read();
}
console.log(s, a.v);
