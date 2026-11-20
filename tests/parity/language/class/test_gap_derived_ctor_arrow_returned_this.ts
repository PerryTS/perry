// `super()` initialises a derived constructor's `this` to the object the base
// constructor returned. An arrow created before `super()` returns shares that
// binding: it observes the returned object, not the discarded receiver.
class Callable {
  constructor(fn: Function, ...[opts]: [{ attach?: object }?]) {
    return Object.assign(
      Object.setPrototypeOf(fn.bind(this), (this as any).constructor.prototype),
      opts?.attach,
    );
  }
}

class Base extends Callable {
  att: any;
  $: any;
  label: string;
  constructor(att: any, $: any) {
    // The arrow is an argument to `super()`: created before `this` is bound.
    super((x: number) => (this as any).go(x), { attach: att });
    this.att = att;
    this.$ = $;
    this.label = "L" + this.$.name;
  }
  go(x: number) {
    return "go:" + this.$.name + x;
  }
}

class Root extends Base {
  constructor(att: any, $: any) {
    super(att, $);
    Object.defineProperty(this, " kind", { value: "root", enumerable: false });
  }
}

const n: any = new Root({ id: 1 }, { name: "S" });
console.log("typeof", typeof n, "label", n.label, "att", n.att.id);
console.log("call", n(2));

// A default parameter reading `this` inside the arrow resolves the same way.
class Hook extends Callable {
  tag = "hook";
  constructor() {
    super((data: string, onFail: () => string = () => (this as any).tag) => data + ":" + onFail(), {});
  }
}
console.log("default param", (new Hook() as any)("d"));

// An arrow nested in an arrow created before `super()` follows the same binding.
class Nested extends Callable {
  tag = "nested";
  constructor() {
    super(() => () => (this as any).tag, {});
  }
}
console.log("nested", (new Nested() as any)()());

// A plain base constructor that returns nothing keeps the receiver.
class Plain {
  tag = "plain";
}
class PlainChild extends Plain {
  read: () => string;
  constructor() {
    const early = () => (this as any).tag;
    super();
    this.read = early;
  }
}
console.log("plain", new PlainChild().read());

// Each construction sees its own returned object.
const a: any = new Root({ id: 1 }, { name: "A" });
const b: any = new Root({ id: 2 }, { name: "B" });
console.log("distinct", a(1), b(1));
