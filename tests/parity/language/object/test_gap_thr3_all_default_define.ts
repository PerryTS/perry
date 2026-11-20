// THR3: defineProperties with all-default attributes (r3c A): the result is
// an ordinary data property; method calls on the instance stay correct.
class Vec {
  x = 1;
}
class AD {
  name: string;
  constructor() {
    Object.defineProperties(this, {
      position: { value: new Vec(), writable: true, enumerable: true, configurable: true },
    });
    this.name = "ad";
  }
  bone(): number {
    return (this as any).position.x;
  }
  getP(): number {
    return this.bone() + 1;
  }
}
let s = 0;
for (let i = 0; i < 10; i++) s += new AD().getP();
const a: any = new AD();
a.position = { x: 5 };
console.log(s, a.getP(), JSON.stringify(Object.getOwnPropertyDescriptor(a, "position")), Object.keys(a).join(","));
