// Issue #3560: object-literal computed methods/accessors with super need a
// home object and must resolve through the current prototype.

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(msg);
}

const proto1 = {
  value() {
    return "one";
  },
  dyn(arg: string) {
    return "d1:" + arg;
  },
  get thing() {
    return "g1";
  },
};

const proto2 = {
  value() {
    return "two";
  },
  dyn(arg: string) {
    return "d2:" + arg;
  },
  get thing() {
    return "g2";
  },
};

const callKey = "call";
const thingKey = "thing";
const obj = {
  [callKey]() {
    return super.value() + "|" + super["dyn"]("x");
  },
  get [thingKey]() {
    return super.thing + "|own";
  },
  method() {
    return super["dyn"]("m");
  },
};

Object.setPrototypeOf(obj, proto1);
assert(obj["call"]() === "one|d1:x", "computed method super call on first prototype");
assert(obj["thing"] === "g1|own", "computed accessor super get on first prototype");
assert(obj.method() === "d1:m", "computed super method call on first prototype");

Object.setPrototypeOf(obj, proto2);
assert(obj["call"]() === "two|d2:x", "computed method super call follows prototype changes");
assert(obj["thing"] === "g2|own", "computed accessor super get follows prototype changes");
assert(obj.method() === "d2:m", "computed super method follows prototype changes");

console.log("issue-3560 ok");
