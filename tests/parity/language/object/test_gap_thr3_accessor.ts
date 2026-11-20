// THR3: an accessor defined in a constructor is not a family data key; the
// method call on the instance still works and reads through the getter.
class V {
  _v: number;
  constructor(v: number) {
    this._v = v;
    Object.defineProperty(this, "v", {
      get() {
        return this._v * 2;
      },
      configurable: true,
    });
  }
  twice(): number {
    return (this as any).v + 1;
  }
}
let s = 0;
for (let i = 0; i < 5; i++) s += new V(i).twice();
const v: any = new V(3);
console.log(s, v.v, typeof Object.getOwnPropertyDescriptor(v, "v")!.get, Object.keys(v).join(","));
