// THR3: a method-named own property defined by defineProperty shadows the
// prototype method (the method guard must miss).
class S {
  k: number;
  constructor(own: boolean) {
    this.k = 1;
    if (own) Object.defineProperty(this, "getP2", { value: () => 42 });
  }
  getP2(): number {
    return 7;
  }
  run(): number {
    return this.getP2();
  }
}
class T {
  constructor() {
    Object.defineProperty(this, "getP2", { value: () => 43 });
  }
  getP2(): number {
    return 8;
  }
  run(): number {
    return this.getP2();
  }
}
let s = 0;
for (let i = 0; i < 6; i++) s += new S(i % 2 === 0).run() * 100 + new T().run();
console.log(s, new S(false).run(), new S(true).run(), new T().run());
