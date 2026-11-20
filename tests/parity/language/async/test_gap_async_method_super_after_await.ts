// `super.m()` / `super.x` inside an async method AFTER an `await`. The
// method's home binding (`super`'s base) must be taken from the called method
// object; an async method runs its body in a state-machine step closure, and
// evaluating the home there named the closure instead, so the lookup missed
// and the call threw "value is not a function". (Next.js:
// NextNodeServer#prepareImpl does `await super.prepareImpl()`.)
class Base {
  dir = "/app";
  am(): string {
    return "Base.am";
  }
  get g(): string {
    return "Base.g";
  }
  async prepareImpl(): Promise<string> {
    return "Base.prepareImpl " + this.dir;
  }
  static s(): string {
    return "Base.s";
  }
}

class Derived extends Base {
  async noAwait() {
    return super.am();
  }
  async afterAwait() {
    await null;
    return super.am();
  }
  async awaited() {
    return await super.prepareImpl();
  }
  async getterAfterAwait() {
    await null;
    return super.g;
  }
  async arrowAfterAwait() {
    await null;
    const f = () => super.am();
    return f();
  }
  async withDefault(x = "d") {
    await null;
    return super.am() + ":" + x;
  }
  async prepareImpl() {
    const before = await super.prepareImpl();
    await this.hook();
    return "Derived(" + before + ")";
  }
  async hook() {
    return "hook";
  }
  static async st() {
    await null;
    return super.s();
  }
  *gen() {
    yield super.am();
  }
}

const d = new Derived();
const cases: [string, () => unknown][] = [
  ["noAwait", () => d.noAwait()],
  ["afterAwait", () => d.afterAwait()],
  ["awaited", () => d.awaited()],
  ["getterAfterAwait", () => d.getterAfterAwait()],
  ["arrowAfterAwait", () => d.arrowAfterAwait()],
  ["withDefault", () => d.withDefault()],
  ["prepareImpl", () => d.prepareImpl()],
  ["static", () => Derived.st()],
  ["gen", () => [...d.gen()]],
];
(async () => {
  for (const [name, f] of cases) {
    try {
      console.log(name, JSON.stringify(await f()));
    } catch (e: any) {
      console.log(name, "threw", e.message);
    }
  }
})();

// The same through a class that captures a local of its enclosing function
// (a CommonJS module body: Next.js's NextNodeServer captures its requires).
// Capture rebinds are inserted ahead of the home binding in the method body.
function make() {
  const helper = { hook: async (d: string) => "hook " + d };
  class B2 {
    async prepareImpl() {
      return "B2";
    }
  }
  class D2 extends B2 {
    dir = "/srv";
    async prepareImpl() {
      const b = await super.prepareImpl();
      return b + "+" + (await helper.hook(this.dir));
    }
    *gen() {
      yield helper.hook.name;
      yield super.prepareImpl.name;
    }
  }
  return new D2();
}
(async () => {
  await null;
  await null;
  const d2 = make();
  try {
    console.log("captured", JSON.stringify(await d2.prepareImpl()));
  } catch (e: any) {
    console.log("captured threw", e.message);
  }
  console.log("captured gen", JSON.stringify([...d2.gen()]));
})();
