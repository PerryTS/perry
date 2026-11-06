// Packed dense loops that read and store `this` number fields: int32 bitwise
// stores (admitted), non-int stores (not admitted), fields holding a
// non-number, and arrays the dense guard must refuse (holes, non-numbers,
// too short for the window).
class D {
  zob: number[] = [];
  hash = 0;
  rights = 15;
  f = 0.5;
  constructor(len: number, hole: boolean) {
    for (let i = 0; i < len; i++) this.zob.push(i * 2654435761 % 1000003);
    if (hole) delete (this.zob as any)[17];
  }
  mix(n: number): number {
    for (let i = 0; i < n; i++) { this.hash ^= this.zob[i & 1023] ?? 0; this.rights &= ~(i & 3); this.rights |= i & 15; }
    return this.hash + this.rights;
  }
  frac(n: number): number { for (let i = 0; i < n; i++) this.f = this.f * 0.5 + (this.zob[i & 1023] ?? 1); return this.f; }
}
for (const [len, hole] of [[1024, false], [1024, true], [600, false], [2048, false]] as [number, boolean][]) {
  const d = new D(len, hole);
  console.log(len, hole, d.mix(5000), d.frac(3000));
}
const odd = new D(1024, false);
(odd as any).hash = "str";
console.log(odd.mix(10));
const mixed = new D(1024, false);
(mixed.zob as any)[5] = { valueOf: () => 7 };
console.log(mixed.mix(100));
