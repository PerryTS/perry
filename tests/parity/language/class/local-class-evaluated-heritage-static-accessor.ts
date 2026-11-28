// A function-local class whose heritage is evaluated (`extends ns.Field`,
// `extends Base` for a value) gets a fresh class object per evaluation; its
// own static accessors still answer before the parent's (#15178). This is
// the shape a CommonJS package compiles to: its module body is a function,
// so cron-parser's `class CronSecond extends CronField_1.CronField` with
// `static get min()` read CronField's throwing getter.
function make(base: any, tag: string) {
  const ns = { Field: base };
  class Second extends ns.Field {
    static get min() {
      return `${tag}:min`;
    }
    static get self() {
      return this === Second;
    }
    static set only(value: string) {}
    static plain() {
      return `${tag}:plain`;
    }
  }
  return Second;
}

class Field {
  static get min(): string {
    throw new Error("min must be overridden");
  }
  static get max() {
    return "Field:max";
  }
  static get only() {
    return "Field:only";
  }
  static plain() {
    return "Field:plain";
  }
}
const Other: any = class {
  static get min() {
    return "Other:min";
  }
  static get max() {
    return "Other:max";
  }
};

const A: any = make(Field, "A");
const B: any = make(Other, "B");
for (const [name, C] of [["A", A], ["B", B]] as const) {
  try {
    console.log(name, "min", C.min);
  } catch (error) {
    console.log(name, "min throws", String(error));
  }
  console.log(name, "max", C.max, "self", C.self, "plain", C.plain());
  console.log(name, "only (setter without getter)", C.only);
}
console.log("A again", A.min, "B again", B.min);
