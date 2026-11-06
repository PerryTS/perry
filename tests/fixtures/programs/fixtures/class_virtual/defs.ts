export class Base {
  scale: number;
  constructor(k: number) { this.scale = k; }
  step(x: number): number { return (x & 255) * this.scale + 7; }
}
export class Derived extends Base {
  step(x: number): number { return (x & 255) * this.scale + 11; }
}
export class Third extends Base {
  step(x: number): number { return (x & 255) * this.scale + 13; }
}
export class Fourth extends Base {
  step(x: number): number { return (x & 255) * this.scale + 17; }
}
export class Inherited extends Derived { extra = 99; }
export class SuperStep extends Derived {
  step(x: number): number { return super.step(x) + 19; }
}
