// THR3: a [[Set]] through a compiled static-key store (`o.q = v`, the store
// probe) of a key the constructor defines later, writable but not
// enumerable or configurable. The [[Set]] adds a plain property (writable,
// enumerable, configurable); the later definition only changes its value,
// so the key stays enumerable and configurable.
function poke(o: any, v: number): void {
  o.q = v;
}
class Late {
  k: number;
  constructor(mode: number) {
    this.k = 1;
    if (mode === 1) poke(this, 5);
    Object.defineProperty(this, "q", { value: 10 + mode, writable: true });
  }
}
const out: string[] = [];
for (let round = 0; round < 3; round++) {
  for (let mode = 0; mode <= 1; mode++) {
    const o: any = new Late(mode);
    const d = Object.getOwnPropertyDescriptor(o, "q")!;
    out.push(mode + ":" + o.q + ":" + d.writable + d.enumerable + d.configurable + ":" + Object.keys(o).join("|"));
    poke(o, 20 + mode);
    out.push("" + o.q);
  }
}
console.log(out.join(" "));
