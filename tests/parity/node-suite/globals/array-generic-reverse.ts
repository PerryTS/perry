function displayLength(value: any): number {
  const n = Number(value.length);
  if (!Number.isFinite(n) || n <= 0) {
    return 0;
  }
  return Math.trunc(n);
}

function show(label: string, value: any): void {
  const parts: string[] = [];
  for (let i = 0; i < displayLength(value); i++) {
    parts.push(Object.hasOwn(value, i) ? String(value[i]) : "<hole>");
  }
  const keys = Object.getOwnPropertyNames(value).sort().join("|");
  console.log(label + ": " + parts.join(",") + " keys=" + keys);
}

function showError(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ": no throw");
  } catch (e: any) {
    console.log(label + ": " + e.constructor.name);
  }
}

const sparse: any = { 0: "a", 2: "c", 4: "e", length: 5 };
const sparseRet = Array.prototype.reverse.call(sparse);
console.log("sparse return same: " + (sparseRet === sparse));
show("sparse", sparse);

const oneSided: any = { 1: "b", length: 4 };
Array.prototype.reverse.call(oneSided);
show("one-sided", oneSided);

const coercedLength: any = { 0: "a", 1: "b", 2: "c", 3: "d", length: "4.9" };
Array.prototype.reverse.call(coercedLength);
show("coerced length", coercedLength);

const applyTarget: any = { 0: "w", 1: "x", 2: "y", 3: "z", length: 4 };
const applyRet = Array.prototype.reverse.apply(applyTarget, []);
console.log("apply return same: " + (applyRet === applyTarget));
show("apply", applyTarget);

const deleted: any = { 0: "a", length: 3 };
Array.prototype.reverse.call(deleted);
show("delete missing source", deleted);
console.log("delete has: " + Object.hasOwn(deleted, 0) + " " + Object.hasOwn(deleted, 2));

showError("null receiver", () => Array.prototype.reverse.call(null));
showError("undefined receiver", () => Array.prototype.reverse.call(undefined));
showError("string abc receiver", () => Array.prototype.reverse.call("abc"));

const one = Array.prototype.reverse.call("a") as any;
console.log("string one: " + typeof one + " " + one.length + " \"" + one.valueOf() + "\"");

const empty = Array.prototype.reverse.call("") as any;
console.log("string empty: " + typeof empty + " " + empty.length + " \"" + empty.valueOf() + "\"");
