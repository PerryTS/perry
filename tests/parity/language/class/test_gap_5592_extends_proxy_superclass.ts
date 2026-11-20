// #5592: `class X extends <Proxy>` must not crash. A Proxy is NaN-boxed over a
// small registry id (handle band), so the runtime's bound-native-method probe
// used to dereference it as a closure header and segfault. IsConstructor must
// be resolved through the proxy target: a proxy wrapping a non-constructor
// (arrow/async/generator) throws a TypeError at class-definition time, without
// reading `.prototype`; a proxy wrapping a real constructor is a valid base.
function check(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ": no throw");
  } catch (e) {
    console.log(label + ": " + (e as Error).constructor.name);
  }
}

const arrowProxy: any = new Proxy((): void => {}, {
  get(): never {
    throw new Error("prototype must not be read");
  },
});
check("extends proxy-of-arrow", () => {
  class A extends arrowProxy {}
  void A;
});

function Base(this: any): void {}
const ctorProxy: any = new Proxy(Base, {});
check("extends proxy-of-constructor", () => {
  class B extends ctorProxy {}
  void B;
});
