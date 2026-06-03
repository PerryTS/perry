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
