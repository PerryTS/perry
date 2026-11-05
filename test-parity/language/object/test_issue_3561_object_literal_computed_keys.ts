// Issue #3561: object literal computed keys should use ToPropertyKey once,
// preserve symbol identity, overwrite duplicates, and expose numeric keys in
// canonical property order.

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(msg);
}

const sym = Symbol("issue-3561");
let toStringCalls = 0;
const objKey = {
  toString() {
    toStringCalls++;
    return "obj-key";
  },
};

const obj = {
  [sym]: "symbol-value",
  [1]: "one",
  ["1"]: "one-overwritten",
  [null as any]: "null-value",
  [true as any]: "true-value",
  [objKey as any]: "object-value",
  get ["acc"]() {
    return "accessor:" + this["1"];
  },
  set ["sink"](v: string) {
    this.saved = v;
  },
  ["m"]() {
    return this.saved || "none";
  },
};

(obj as any).sink = "saved";

assert(toStringCalls === 1, "computed object key coerced exactly once");
assert((obj as any)[sym] === "symbol-value", "symbol key identity preserved");
assert(obj["1"] === "one-overwritten", "duplicate stringified key overwrites");
assert((obj as any)["null"] === "null-value", "null key stringified");
assert((obj as any)["true"] === "true-value", "boolean key stringified");
assert((obj as any)["obj-key"] === "object-value", "object key stringified");
assert((obj as any)["acc"] === "accessor:one-overwritten", "computed accessor works");
assert((obj as any)["m"]() === "saved", "computed method this binding works");
assert(Object.getOwnPropertySymbols(obj)[0] === sym, "own symbol key is retained");

const ordered = {
  ["10"]: "ten",
  [2]: "two",
  ["1"]: "one",
  a: "a",
};
assert(Object.keys(ordered).join(",") === "1,2,10,a", "numeric keys use canonical order");

console.log("issue-3561 ok");
