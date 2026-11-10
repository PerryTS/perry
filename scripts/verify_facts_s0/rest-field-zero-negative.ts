// `Sign.z` holds `-0` (a product of 0 and a negative Number), which no I32
// can hold. Under PERRY_FACTS_SABOTAGE=restfield the facts claim the field
// I32. The witness at the proven field read must report the `-0` word.
class Sign {
  z = 1;
  set(a: number, b: number): void {
    this.z = a * b;
  }
  read(): number {
    return this.z | 0;
  }
}
const s = new Sign();
const n = process.argv.length > 100 ? 1 : -1;
let t = 0;
for (let i = 0; i < 4; i++) {
  s.set(i - i, n);
  t += s.read();
}
console.log(t, Object.is(s.z, -0));
