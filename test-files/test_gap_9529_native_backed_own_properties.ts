// Regression for #9529: native-backed instances must retain ordinary own
// properties instead of silently dropping their definitions.

const accessor: any = new Uint8Array(1);
Object.defineProperty(accessor, "g", {
  get() {
    return 1;
  },
  configurable: true,
});
console.log(
  "typed-array accessor",
  accessor.g,
  Object.getOwnPropertyDescriptor(accessor, "g") !== undefined,
);

const shorthand: any = new Uint8Array(1);
Object.defineProperty(shorthand, "g", {
  get() {
    return 2;
  },
});
console.log("typed-array shorthand", shorthand.g);

const data: any = new Uint8Array(1);
Object.defineProperty(data, "d", { value: 5, enumerable: true });
console.log("typed-array data", data.d, Object.keys(data).includes("d"));

const date: any = new Date(0);
date.toISOString = function () {
  return "iso";
};
console.log(
  "date override",
  JSON.stringify(date),
  JSON.stringify({ date }),
  JSON.stringify([date]),
  JSON.stringify({ date }, null, 2).includes('"iso"'),
  date.toJSON(),
);

const builtinDate = new Date(0);
console.log("date builtin", JSON.stringify(builtinDate), builtinDate.toJSON());
