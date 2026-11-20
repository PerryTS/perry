// Error, Date and RegExp instances keep their expando properties in side
// tables. `Object.keys` read them there, but `Object.values` and
// `Object.entries` did not, so both returned [] for every Error subclass
// instance with fields. A class that captures a local of its defining
// function (Effect's `Data.TaggedError(tag)` sets `_tag = tag`) also stores
// the capture on the instance; on an Error subclass that compiler-owned slot
// showed up in `Object.keys`, `JSON.stringify` and `getOwnPropertyNames`.
class FieldError extends Error {
  x = 1;
}
const e: any = new FieldError("m");
e.y = 2;
console.log("fields", Object.keys(e), Object.values(e), Object.entries(e));
console.log("fields json", JSON.stringify(e), Object.getOwnPropertyNames(e));

const TaggedError = (tag: string): any => {
  class Base extends Error {
    readonly _tag = tag;
  }
  return Base;
};
class PlatformError extends TaggedError("PlatformError") {
  reason: string;
  constructor(reason: string) {
    super();
    this.reason = reason;
  }
}
const p: any = new PlatformError("NotFound");
console.log("tagged", Object.keys(p), Object.values(p), Object.entries(p));
console.log("tagged json", JSON.stringify(p), Object.getOwnPropertyNames(p));
console.log("tagged spread", JSON.stringify({ ...p }));

const d: any = new Date(0);
d.tag = "d";
console.log("date", Object.keys(d), Object.values(d), Object.entries(d));
const re: any = /x/g;
re.tag = "r";
console.log("regexp", Object.keys(re), Object.values(re), Object.entries(re));
