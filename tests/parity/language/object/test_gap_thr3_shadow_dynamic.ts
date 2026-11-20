// THR3: the shadowing key is computed at run time (the compiler cannot name
// it): the own property still shadows the prototype method, also when its
// attributes are all default.
const name = ["get", "P2"].join(process.argv.length > 100 ? "x" : "");
class D {
  k: number;
  constructor(mode: number) {
    this.k = 1;
    if (mode === 1) Object.defineProperty(this, name, { value: () => 42 });
    if (mode === 2)
      Object.defineProperty(this, name, { value: () => 43, writable: true, enumerable: true, configurable: true });
    if (mode === 3) (this as any)[name] = () => 44;
  }
  getP2(): number {
    return 7;
  }
  run(): number {
    return this.getP2();
  }
}
const out: number[] = [];
for (let r = 0; r < 3; r++) for (let m = 0; m < 4; m++) out.push(new D(m).run());
const late: any = new D(0);
const before = late.run();
Object.defineProperty(late, name, { value: () => 45 });
console.log(out.join(","), before, late.run());
