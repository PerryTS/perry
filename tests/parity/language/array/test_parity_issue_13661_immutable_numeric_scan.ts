// Private constant arrays may fold only own, proven in-bounds reads.
const directions: number[] = [-17, -16, -15, -1, 1, 15, 16, 17];
function scan(origin: number, fixed: boolean): number {
  let sum = 0;
  if (fixed) {
    for (let j = 0; j < 8; j += 1) sum += origin + (directions[j] ?? 0);
  } else {
    for (let j = 0; j < directions.length; j++) sum += origin + (directions[j] ?? 0);
  }
  return sum;
}
for (const origin of [-2147483648, -0, 0.5, 127, 2147483648, Infinity, NaN]) {
  console.log("scan", scan(origin, false), scan(origin, true));
}

// Length must remain live when any alias or callback can mutate the array.
const changing: number[] = [1, 2, 3, 4];
const alias = changing;
function shrink() { alias.pop(); }
let total = 0;
for (let i = 0; i < changing.length; i += 1) {
  total += changing[i];
  shrink();
}
console.log("mutation", total, changing.length);

const described: number[] = [1, 2, 3];
Object.defineProperty(described, "1", { get() { described.length = 2; return 9; } });
total = 0;
for (let i = 0; i < described.length; i++) total += described[i];
console.log("descriptor", total, described.length);

const deleted: number[] = [1, 2, 3];
delete deleted[1];
Object.defineProperty(Array.prototype, "1", { configurable: true, get() { return 11; } });
let inherited = 0;
for (let i = 0; i < deleted.length; i++) inherited += deleted[i];
delete (Array.prototype as any)[1];
console.log("inherited", inherited);

const lies: number[] = [1, "x", 3] as any;
let concatenated: any = 0;
for (let i = 0; i < lies.length; i += 1) concatenated += lies[i];
console.log("erased", concatenated);

const exact: number[] = [-0, NaN, Infinity, 2147483648];
for (let i = 0; i < exact.length; i++) console.log("exact", Object.is(exact[i], -0), exact[i]);
const keys: number[] = [7, 8];
console.log("keys", keys[-0], keys[0.5], keys[NaN], keys[2147483648], keys["0"]);

function beforeInit() { return later[0]; }
try { console.log(beforeInit()); } catch (e) { console.log("tdz", e instanceof ReferenceError); }
const later: number[] = [41, 42];
console.log("after", beforeInit());

// Reassigning the induction variable invalidates the bounded-read proof.
const bounds: number[] = [10, 20, 30];
for (let i = 0; i < bounds.length; i += 1) {
  i += 2;
  console.log("counter", bounds[i]);
}

// Even a syntactically private array escapes through an inherited getter's
// receiver. One unproven indexed read must keep ALL its reads and length live.
const inheritedReceiver: number[] = [1, 2];
Object.defineProperty(Array.prototype, "9", {
  configurable: true,
  get() { this[0] = 99; this.length = 1; return 5; }
});
const inheritedResult = inheritedReceiver[9];
delete (Array.prototype as any)[9];
console.log("getter-receiver", inheritedResult, inheritedReceiver.length, inheritedReceiver[0]);
console.log("string-origin", scan("x" as any, false), scan("x" as any, true));

let conditions = 0;
function choose(flag: boolean): number {
  const value = (++conditions, flag) ? -0 : NaN;
  return value;
}
console.log("selection", Object.is(choose(true) + -0, -0), Number.isNaN(choose(false)), conditions);
function mutatingCondition(): any {
  const values = [10, 20, 30];
  let offset = 0;
  if (offset >= 0 && offset < 1) {
    const index = ((offset = 0.5, true) ? 1 : 2) + offset;
    return values[index];
  }
}
console.log("condition-mutation", mutatingCondition());
