export class Base {
  x: number = 1;
  area(): number { return 0; }
  describe(): number { return this.area() + this.x; }
}
export class Other extends Base {
  area(): number { return 5; }
}
export function makeSub(P: new () => Base): new () => Base {
  return class extends P {
    area(): number { return 6; }
  };
}
