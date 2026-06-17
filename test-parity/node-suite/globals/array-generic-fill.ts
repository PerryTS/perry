const hasOwn = Object.prototype.hasOwnProperty;

function slot(value: any): string {
  return value === undefined ? "undefined" : String(value);
}

function dump(label: string, value: any): void {
  console.log(
    `${label}.values:${slot(value[0])}|${slot(value[1])}|${slot(value[2])}|${slot(value[3])}|${slot(value[4])}`,
  );
  console.log(
    `${label}.has:${hasOwn.call(value, "0")}|${hasOwn.call(value, "1")}|${hasOwn.call(value, "2")}|${hasOwn.call(value, "3")}|${hasOwn.call(value, "4")}`,
  );
  console.log(`${label}.length:${value.length}`);
}

const rangeReceiver: any = { 0: "a", 2: "c", 4: "e", length: 5 };
const rangeReturned = Array.prototype.fill.call(rangeReceiver, "x", 1, -1);
console.log(`range.same:${rangeReturned === rangeReceiver}`);
dump("range", rangeReceiver);

const negativeReceiver: any = { 0: "a", 1: "b", 2: "c", length: 3 };
Array.prototype.fill.call(negativeReceiver, "z", -2);
dump("negative", negativeReceiver);

const infiniteReceiver: any = { length: 4 };
Array.prototype.fill.call(infiniteReceiver, "q", -Infinity, Infinity);
dump("infinite", infiniteReceiver);

const stringCoercedReceiver: any = { 0: "a", 3: "d", length: "4" };
Array.prototype.fill.call(stringCoercedReceiver, "s", "1", "3");
dump("string", stringCoercedReceiver);

try {
  Array.prototype.fill.call(null, "n");
  console.log("nullish:no-throw");
} catch (err: any) {
  console.log(`nullish:${err.name}`);
}
