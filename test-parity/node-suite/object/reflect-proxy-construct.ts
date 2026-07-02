function show(label: string, fn: () => unknown) {
  try {
    console.log(label, "ok", JSON.stringify(fn()));
  } catch (err: any) {
    console.log(label, "throw", err?.constructor?.name ?? err?.name ?? typeof err);
  }
}

function Target(a: number, b: number) {
  (this as any).sum = a + b;
  (this as any).args = [a, b];
}

function NewTarget() {}
(NewTarget as any).prototype.kind = "custom";

show("Reflect.construct newTarget prototype", () => {
  const obj: any = Reflect.construct(Target, [2, 3], NewTarget);
  return [
    obj.sum,
    Object.getPrototypeOf(obj) === (NewTarget as any).prototype,
    obj instanceof (NewTarget as any),
  ];
});

show("Reflect.construct array-like args", () => {
  const obj: any = Reflect.construct(Target, { 0: 4, 1: 5, length: 2 } as any);
  return obj.sum;
});

show("Reflect.construct null args throws", () => {
  return Reflect.construct(Target, null as any);
});

show("Reflect.construct nonconstructor target throws", () => {
  return Reflect.construct(1 as any, []);
});

show("Reflect.construct nonconstructor newTarget throws", () => {
  return Reflect.construct(Target, [], 1 as any);
});

function TrapTarget(a: string) {
  (this as any).arg = a;
}

let proxyWithTrap: any;
const handler = {
  construct(target: any, args: any[], newTarget: any) {
    return [args[0], newTarget === proxyWithTrap, this === handler, target === TrapTarget];
  },
};
proxyWithTrap = new Proxy(TrapTarget, handler);

show("proxy construct trap args", () => {
  return Reflect.construct(proxyWithTrap, ["p"]);
});

const badReturn = new Proxy(function BadReturn() {}, {
  construct() {
    return 1 as any;
  },
});

show("proxy construct trap bad return throws", () => {
  return Reflect.construct(badReturn, []);
});

function Foo(a: number) {
  (this as any).arg = a;
}

function Bar() {}
(Bar as any).prototype = Object.create((Foo as any).prototype);
(Bar as any).prototype.constructor = Bar;
(Bar as any).prototype.isBar = true;

const FooProxy: any = new Proxy(new Proxy(Foo, {}), { construct: null as any });

show("proxy chain honors newTarget prototype", () => {
  const bar: any = Reflect.construct(FooProxy, [7], Bar);
  return [
    bar.arg,
    Object.getPrototypeOf(bar) === (Bar as any).prototype,
  ];
});

class Thing {
  value: string;
  constructor(value: string) {
    this.value = value;
  }
  method() {
    return `m:${this.value}`;
  }
}

show("proxy class empty handler", () => {
  const Wrapped: any = new Proxy(Thing, {});
  const instance: any = new Wrapped("x");
  return [instance instanceof Thing, instance.method()];
});

const classConstructHandler = {
  construct(target: any, args: any[], newTarget: any) {
    const instance: any = Reflect.construct(target, args, newTarget);
    instance.value = `${instance.value}:proxy`;
    return instance;
  },
};
const WrappedThingWithTrap: any = new Proxy(Thing, classConstructHandler);

show("proxy class construct trap delegates", () => {
  const instance: any = new WrappedThingWithTrap("x");
  return [instance instanceof Thing, instance.value, instance.method()];
});

class Child extends Thing {
  child() {
    return `child:${this.value}`;
  }
}

show("proxy class empty handler newTarget", () => {
  const Wrapped: any = new Proxy(Thing, {});
  const instance: any = Reflect.construct(Wrapped, ["y"], Child);
  return [instance instanceof Child, instance instanceof Thing, instance.child()];
});

class CtorNewTarget {
  ntName: string;
  constructor() {
    this.ntName = (new.target as any)?.name ?? "undefined";
  }
}
class OtherNewTarget {}

show("Reflect.construct new.target inside class ctor", () => {
  const a: any = Reflect.construct(CtorNewTarget, [], OtherNewTarget);
  return a.ntName;
});

show("ClassRef new new.target inside class ctor", () => {
  const Ref: any = CtorNewTarget;
  const a: any = new Ref();
  return a.ntName;
});

show("static new new.target inside base class ctor", () => {
  return new CtorNewTarget().ntName;
});

function freeProbe(): string {
  return (new.target as any) === undefined ? "undef" : "leaked";
}
class CallsFreeProbe {
  probed: string;
  constructor() {
    this.probed = freeProbe();
  }
}

show("free fn called from static-new ctor sees undefined new.target", () => {
  return new CallsFreeProbe().probed;
});

// #2768: a subclass whose OWN ctor body never reads `new.target` still runs the
// inherited base ctor (via `super()`) inlined into its standalone symbol. The
// base reads `new.target`, so `new Child()` must observe `Child`, not undefined
// — the symbol-call new.target gate must span the whole super(...) chain.
class NtBase {
  ntName: string;
  constructor() {
    this.ntName = (new.target as any)?.name ?? "undefined";
  }
}
class NtChild extends NtBase {
  extra: number;
  constructor() {
    super();
    this.extra = 1;
  }
}
class NtNoCtorChild extends NtBase {}

show("static new on own-ctor subclass: base ctor sees leaf new.target", () => {
  return new NtChild().ntName;
});

show("static new on no-own-ctor subclass: base ctor sees leaf new.target", () => {
  return new NtNoCtorChild().ntName;
});

// An abstract-class guard living in the base must still fire for `new Base()`
// but NOT for `new Sub()` whose own ctor forwards through `super()`.
class AbstractBase {
  constructor() {
    if (new.target === AbstractBase) throw new TypeError("abstract");
  }
}
class ConcreteSub extends AbstractBase {
  tag: number;
  constructor() {
    super();
    this.tag = 7;
  }
}

show("abstract-base guard: new Sub() does not trip base new.target guard", () => {
  return new ConcreteSub().tag;
});

show("abstract-base guard: new Base() trips base new.target guard", () => {
  return new AbstractBase();
});
