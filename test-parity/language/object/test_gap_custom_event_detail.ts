// `new CustomEvent(type, { detail })` and `new Event(type, options)`: the
// event keeps `detail` (a write through `e.detail` changes the object given),
// and the options' getters and the type's toString run.
class V {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
  add(o: V): V {
    return new V(this.x + o.x);
  }
}
class W extends V {
  constructor(x: number) {
    super(x * 10);
  }
  add(o: V): V {
    return new W(this.x + o.x);
  }
}
const g = globalThis as any;

class Plain {
  v: V;
  constructor() {
    this.v = new V(1);
    if (typeof g.__devtools !== "undefined") {
      g.__devtools.dispatchEvent(new CustomEvent("observe", { detail: this }));
    }
  }
}

class ThroughDetail {
  v: V;
  constructor() {
    this.v = new V(1);
    const e: any = new CustomEvent("observe", { detail: this });
    e.detail.v = new W(2);
    console.log(e.type, e.detail === this, e.bubbles);
  }
}

class Getter {
  v: V;
  constructor() {
    this.v = new V(1);
    const self = this;
    const e = new CustomEvent("observe", {
      get detail() {
        self.v = new W(3);
        return 7;
      },
    });
    console.log(e.detail);
  }
}

class EventGetter {
  v: V;
  constructor() {
    this.v = new V(1);
    const self = this;
    const e = new Event("observe", {
      get bubbles() {
        self.v = new W(4);
        return true;
      },
    });
    console.log(e.bubbles);
  }
}

class TypeToString {
  v: V;
  constructor() {
    this.v = new V(1);
    const self = this;
    const e = new CustomEvent({
      toString() {
        self.v = new W(5);
        return "t";
      },
    } as any);
    console.log(e.type, e.detail);
  }
}

for (const C of [Plain, ThroughDetail, Getter, EventGetter, TypeToString]) {
  const o = new C();
  console.log(C.name, o.v.add(new V(100)).x, o.v instanceof W);
}
