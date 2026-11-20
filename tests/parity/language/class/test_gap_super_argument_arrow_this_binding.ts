// An arrow passed to `super(...)` reads the derived constructor's `this`
// binding, which `super()` sets to the object the parent constructor returned.
// ArkType's `GenericRoot` passes such an arrow to `Callable`, whose
// constructor returns a re-prototyped bound function, and calls it later.
class Callable {
  constructor(fn: (...a: any[]) => any) {
    return Object.setPrototypeOf(fn.bind(this), new.target.prototype);
  }
}
class Generic extends Callable {
  params: number[];
  constructor(params: number[]) {
    super((...args: number[]) => this.params.map((p) => p + args.length));
    this.params = params;
  }
}
const g: any = new Generic([1, 2]);
console.log("callable result:", JSON.stringify(g(7, 8)));
console.log("arrow this is the instance:", typeof g, g instanceof Generic);

class Nested extends Callable {
  tag = "nested";
  constructor() {
    super(() => [1, 2].map(() => this.tag).join(","));
  }
}
console.log("nested arrow:", (new Nested() as any)());

class Holder {
  cb: () => any;
  constructor(cb: () => any) { this.cb = cb; }
}
class Plain extends Holder {
  v = 5;
  constructor() { super(() => this.v); }
}
const p = new Plain();
console.log("plain base:", p.cb(), p.cb() === p.v);

class Branchy extends Holder {
  w: string;
  constructor(flag: boolean) {
    if (flag) {
      super(() => this.w + "!");
    } else {
      super(() => this.w + "?");
    }
    this.w = flag ? "yes" : "no";
  }
}
console.log("branches:", new Branchy(true).cb(), new Branchy(false).cb());
