// THR3: a [[Set]] of a key the constructor defines later adds a plain
// property (writable, enumerable, configurable) whichever path performs it:
// an outside function, a computed key, Reflect.set or Object.assign. The
// later definition then redefines that configurable property.
function poke(o: any): void {
  o.q = 5;
}
function pokeKey(o: any, k: string): void {
  o[k] = 6;
}
class Pre {
  k: number;
  constructor(mode: number) {
    this.k = 1;
    if (mode === 1) poke(this);
    else if (mode === 2) pokeKey(this, "q");
    else if (mode === 3) Reflect.set(this, "q", 7);
    else if (mode === 4) Object.assign(this, { q: 8 });
    Object.defineProperty(this, "q", { value: 10 + mode });
  }
}
const out: string[] = [];
for (let round = 0; round < 2; round++) {
  for (let mode = 0; mode <= 4; mode++) {
    try {
      const o: any = new Pre(mode);
      const d = Object.getOwnPropertyDescriptor(o, "q")!;
      out.push(mode + ":" + o.q + ":" + d.writable + d.enumerable + d.configurable + ":" + Object.keys(o).join("|"));
    } catch (e) {
      out.push(mode + ":" + (e as Error).constructor.name);
    }
  }
}
console.log(out.join(" "));
