// Built-in namespace own-property descriptors: methods and constants should be
// visible through Object.getOwnPropertyDescriptor/Object.getOwnPropertyNames.

function valueShape(value: any): string {
  if (typeof value === "function") {
    return "function:" + value.name + ":" + value.length;
  }
  return typeof value + ":" + String(value);
}

function showDesc(label: string, obj: any, key: any) {
  const desc = Object.getOwnPropertyDescriptor(obj, key);
  if (!desc) {
    console.log(label + ": missing");
    return;
  }
  if ("value" in desc) {
    console.log(
      label +
        ": data:" +
        valueShape(desc.value) +
        ":" +
        desc.writable +
        ":" +
        desc.enumerable +
        ":" +
        desc.configurable,
    );
    return;
  }
  console.log(
    label +
      ": accessor:" +
      typeof desc.get +
      ":" +
      typeof desc.set +
      ":" +
      desc.enumerable +
      ":" +
      desc.configurable,
  );
}

const mathSubset = Object.getOwnPropertyNames(Math).filter((name) =>
  ["abs", "random", "E", "PI", "f16round"].includes(name),
);
console.log("Math names subset:", mathSubset.join(","));

showDesc("Math.abs", Math, "abs");
showDesc("Math.random", Math, "random");
showDesc("Math.f16round", Math, "f16round");
showDesc("Math.E", Math, "E");
showDesc("Math.PI", Math, "PI");

if (typeof Math.abs === "function") {
  showDesc("Math.abs.name", Math.abs, "name");
  showDesc("Math.abs.length", Math.abs, "length");
}
