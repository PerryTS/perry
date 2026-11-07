// `this.f op= v` reads and writes `this` in place (no spilled base temp).
// Evaluation order, accessors, logical forms, TDZ and loops must keep their
// semantics.
class P {
  x = 1;
  s = "a";
  n: number | null = null;
  z = 0;
  log: string[] = [];
  private _v = 10;
  get v(): number { this.log.push("get"); return this._v; }
  set v(y: number) { this.log.push("set " + y); this._v = y; }
  order(): number[] {
    // The read happens before the RHS: the RHS's write is overwritten.
    this.x += (this.x = 100, 1);
    const a = this.x;
    this.x *= ((): number => { this.x = 7; return 3; })();
    return [a, this.x];
  }
  accessor(): string {
    this.v += 5;
    this.v **= 2;
    return this.log.join(",") + " " + this.v;
  }
  logical(): any[] {
    this.n ??= 4;
    this.n ??= 9;
    this.z ||= 8;
    this.x &&= 0;
    this.s ||= "never";
    return [this.n, this.z, this.x, this.s];
  }
  loop(k: number): any[] {
    for (let i = 0; i < k; i++) {
      this.z += i & 7;
      this.s += i % 3 === 0 ? "b" : "";
      this.x ^= i;
      this.x <<= 1;
      this.x >>>= 1;
      this.x %= 1000003;
    }
    return [this.z, this.s.length, this.x];
  }
}
class Base { y = 2; }
class Derived extends Base {
  out: string;
  constructor() {
    let caught = "none";
    try {
      // @ts-ignore: deliberate TDZ read
      (this as any).y += 1;
    } catch (e) {
      caught = (e as Error).constructor.name;
    }
    super();
    this.y += 40;
    this.out = caught;
  }
}
const p = new P();
console.log(JSON.stringify(p.order()));
console.log(p.accessor());
console.log(JSON.stringify(p.logical()));
console.log(JSON.stringify(new P().loop(10000)));
const d = new Derived();
console.log(d.out, d.y);
// A method borrowed by another receiver updates that receiver.
const other: any = { z: 1, s: "q", x: 3 };
console.log(JSON.stringify(P.prototype.loop.call(other, 50)), other.z);
