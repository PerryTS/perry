// A class declaration inside a lexical loop body must snapshot that
// iteration's loop-head binding.  Class methods and field initializers can run
// after the loop has advanced (or ended), so sharing the class capture table
// across iterations makes every instance observe the final value.

const classicImmediate: string[] = [];
const classicDelayed: Array<{ method(): string; field: string }> = [];

for (let i = 0; i < 3; i++) {
  class Classic {
    field = "field-" + i;

    method(): string {
      return "method-" + i;
    }
  }

  const instance = new Classic();
  classicImmediate.push(instance.method() + "/" + instance.field);
  classicDelayed.push(instance);
}

console.log("classic immediate:", classicImmediate.join(","));
console.log(
  "classic delayed methods:",
  classicDelayed.map((instance) => instance.method()).join(","),
);
console.log(
  "classic delayed fields:",
  classicDelayed.map((instance) => instance.field).join(","),
);

const forOfDelayed: Array<{ method(): string; field: string }> = [];

for (const value of ["a", "b", "c"]) {
  class ForOf {
    field = "field-" + value;

    method(): string {
      return "method-" + value;
    }
  }

  forOfDelayed.push(new ForOf());
}

console.log(
  "for-of delayed methods:",
  forOfDelayed.map((instance) => instance.method()).join(","),
);
console.log(
  "for-of delayed fields:",
  forOfDelayed.map((instance) => instance.field).join(","),
);

const varDelayed: Array<{ method(): string }> = [];

for (var shared = 0; shared < 3; shared++) {
  class VarControl {
    method(): string {
      return "method-" + shared;
    }
  }

  varDelayed.push(new VarControl());
}

console.log(
  "var control:",
  varDelayed.map((instance) => instance.method()).join(","),
);
