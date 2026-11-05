import util, { inherits } from "node:util";

function Base() {
  this.base = true;
}

Base.prototype.answer = function () {
  return 42;
};

function Sub() {
  Base.call(this);
}

console.log(
  "inherits meta:",
  typeof util.inherits,
  util.inherits.length,
  util.inherits.name,
  util.inherits === inherits,
);
console.log("inherits return:", util.inherits(Sub, Base));

const sub = new Sub();
console.log(
  "inherits result:",
  sub instanceof Sub,
  sub instanceof Base,
  sub.base,
  sub.answer(),
  Sub.super_ === Base,
);
console.log(
  "prototype result:",
  Object.getPrototypeOf(Sub.prototype) === Base.prototype,
);
console.log("sub keys:", Object.keys(Sub).join(",") || "none");

function Leaf() {}
inherits(Leaf, Base);

const leaf = new Leaf();
console.log(
  "named import result:",
  leaf instanceof Leaf,
  leaf instanceof Base,
  typeof leaf.answer,
);
