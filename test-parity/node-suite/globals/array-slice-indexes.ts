const arr: number[] = [1, 2, 3, 4];
const dynamicArr: any = arr;

function item(value: any[], index: number): any {
  return index < value.length ? value[index] : undefined;
}

function dump(label: string, value: any[]) {
  console.log(label, value.length, item(value, 0), item(value, 1), item(value, 2), item(value, 3));
}

function typedSlice(argc: number, start?: any, end?: any) {
  if (argc === 0) return arr.slice();
  if (argc === 1) return arr.slice(start);
  return arr.slice(start, end);
}

function dynamicSlice(argc: number, start?: any, end?: any) {
  if (argc === 0) return dynamicArr.slice();
  if (argc === 1) return dynamicArr.slice(start);
  return dynamicArr.slice(start, end);
}

function prototypeCallSlice(argc: number, start?: any, end?: any) {
  const slice = Array.prototype.slice;
  if (argc === 0) return slice.call(arr);
  if (argc === 1) return slice.call(arr, start);
  return slice.call(arr, start, end);
}

const cases: Array<[string, number, any?, any?]> = [
  ["no args", 0],
  ["start NaN", 1, NaN],
  ["start Infinity", 1, Infinity],
  ["start -Infinity", 1, -Infinity],
  ["start fraction", 1, 1.9],
  ["end Infinity", 2, 1, Infinity],
  ["end NaN", 2, 1, NaN],
  ["end undefined", 2, 1, undefined],
  ["end -Infinity", 2, 1, -Infinity],
];

for (const [label, argc, start, end] of cases) {
  dump("typed " + label, typedSlice(argc, start, end));
}

for (const [label, argc, start, end] of cases) {
  dump("dynamic " + label, dynamicSlice(argc, start, end));
}

for (const [label, argc, start, end] of cases) {
  dump("prototype " + label, prototypeCallSlice(argc, start, end));
}
