type Ctor<T = {}> = new (...args: any[]) => T;

class CoreBase {
  core(): string {
    return "core";
  }
}

function ParamMixin<T>(seed: T) {
  return <TBase extends Ctor>(B: TBase) =>
    class extends B {
      seed: T = seed;

      describeSeed(): string {
        return "seed=" + String(this.seed);
      }
    };
}

class Combined extends ParamMixin<number>(42)(CoreBase) {}

const combined = new Combined();
console.log("combo.core:", combined.core());
console.log("combo.seed:", combined.seed);
console.log("combo.describeSeed:", combined.describeSeed());
