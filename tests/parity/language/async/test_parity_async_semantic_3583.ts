async function returnsValue() { return 42; }
function failDefault(): number { throw "default-boom"; }
function failArrowDefault(): number { throw "arrow-default-boom"; }
async function rejectsDefault(value = failDefault()) { console.log("default body ran"); return value; }
async function throwsNow() { throw "throw-boom"; }
async function lengthOne(a: number, b = 39) { return a + b; }
const arrowLengthOne = async (a: number, b = 39) => a + b;
const rejectsArrowDefault = async (value = failArrowDefault()) => {
  console.log("arrow body ran");
  return value;
};

returnsValue().then((value) => console.log("return " + value));
rejectsDefault().then(
  () => console.log("default unexpectedly resolved"),
  (err) => console.log("default rejected " + err),
);
throwsNow().then(
  () => console.log("throw unexpectedly resolved"),
  (err) => console.log("throw rejected " + err),
);
rejectsArrowDefault().then(
  () => console.log("arrow default unexpectedly resolved"),
  (err) => console.log("arrow default rejected " + err),
);
console.log("fn length " + lengthOne.length);
console.log("arrow length " + arrowLengthOne.length);
console.log("sync end");
