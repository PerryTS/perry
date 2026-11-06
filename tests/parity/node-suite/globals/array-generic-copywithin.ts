function dump(label: string, value: any) {
  const parts: string[] = [];
  const len = Number(value.length);
  for (let i = 0; i < len; i++) {
    parts.push(Object.hasOwn(value, i) ? String(value[i]) : "<hole>");
  }
  console.log(label + ": " + parts.join(",") + " keys=" + Object.keys(value).join("|"));
}

function showError(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(label + ": " + err.name);
  }
}

const sparse: any = { length: 5, 0: "a", 1: "b", 3: "d" };
const sparseReturned = Array.prototype.copyWithin.call(sparse, 1, 0, 4);
console.log("sparse return same:", sparseReturned === sparse);
dump("sparse", sparse);

const overlap: any = { length: 5, 0: "a", 1: "b", 2: "c", 3: "d", 4: "e" };
Array.prototype.copyWithin.call(overlap, 1, 0, 4);
dump("overlap backward", overlap);

const negative: any = { length: 5, 0: "a", 1: "b", 2: "c", 3: "d", 4: "e" };
Array.prototype.copyWithin.call(negative, -2, -4, -1);
dump("negative offsets", negative);

const applied: any = { length: 4, 0: "w", 1: "x", 2: "y", 3: "z" };
const appliedReturned = Array.prototype.copyWithin.apply(applied, [0, 2]);
console.log("apply return same:", appliedReturned === applied);
dump("apply", applied);

const deleted: any = { length: 4, 0: "a", 2: "c", 3: "d" };
Array.prototype.copyWithin.call(deleted, 2, 1, 3);
dump("delete missing source", deleted);
console.log("delete has:", Object.hasOwn(deleted, 2), Object.hasOwn(deleted, 3));

const coerced: any = { length: "5", 0: "a", 1: "b", 2: "c", 3: "d", 4: "e" };
Array.prototype.copyWithin.call(coerced, "1", "-Infinity", Infinity);
dump("coerced bounds", coerced);

const noArgs: any = { length: 2, 0: "a", 1: "b" };
const noArgsReturned = Array.prototype.copyWithin.call(noArgs);
console.log("no args return same:", noArgsReturned === noArgs);
dump("no args", noArgs);

showError("null receiver", () => Array.prototype.copyWithin.call(null as any, 0, 1));
showError("undefined receiver", () =>
  Array.prototype.copyWithin.call(undefined as any, 0, 1)
);
showError("string receiver", () => Array.prototype.copyWithin.call("abc" as any, 0, 1));
