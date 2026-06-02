class Logged {
  log: string;

  constructor(seed: string) {
    this.log = "seed=" + seed;
  }
}

type Ctor<T = {}> = new (...args: any[]) => T;

function WithSuffix<TBase extends Ctor<Logged>>(B: TBase) {
  return class extends B {
    constructor(seed: string) {
      super(seed);
      this.log += ":wrapped";
    }
  };
}

class WrappedLogged extends WithSuffix(Logged) {}

console.log("super-args.log:", new WrappedLogged("alpha").log);
