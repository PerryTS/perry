// THR3: every reader of a constructor-defined attributed key sees its
// attributes: stores and deletes throw (class code is strict), copies skip a
// non-enumerable key, and a defined `then` makes the object a thenable.
class R {
  a: number;
  constructor(n: number) {
    this.a = n;
    Object.defineProperty(this, "id", { value: n * 10 });
    (this as any).b = n + 1;
  }
  static store(o: any, k: string): string {
    try {
      o[k] = 99;
      return "stored:" + o[k];
    } catch (e) {
      return (e as Error).constructor.name + ":" + o[k];
    }
  }
  static drop(o: any): string {
    try {
      return String(delete o.id) + ":" + o.id;
    } catch (e) {
      return (e as Error).constructor.name + ":" + o.id;
    }
  }
  static set(o: any): string {
    return String(Reflect.set(o, "id", 5)) + ":" + o.id;
  }
}
class Th {
  v: number;
  constructor(v: number) {
    this.v = v;
    Object.defineProperty(this, "then", { value: (res: (x: number) => void) => res(v * 2) });
  }
}
const out: string[] = [];
for (let i = 1; i <= 3; i++) {
  const r: any = new R(i);
  out.push(R.store(r, "id"), R.store(r, "a"), R.drop(r), R.set(r));
  out.push(JSON.stringify(r), Object.keys(Object.assign({}, r)).join("|"));
  out.push(Object.entries(r).join("|"), Object.values(r).join("|"));
  out.push(String(r.propertyIsEnumerable("id")), String(Object.isFrozen(Object.freeze(r))));
  out.push(R.store(r, "a"), R.store(r, "zz"));
}
console.log(out.join(" "));
Promise.resolve(new Th(21)).then((x: any) => console.log("then", typeof x === "number" ? x : "object"));
(async () => {
  const x: any = await new Th(5);
  console.log("await", typeof x === "number" ? x : "object");
})();
