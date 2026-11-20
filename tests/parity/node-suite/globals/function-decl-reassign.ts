function G() {
  return "call-old";
}

const alias = G;

G = function () {
  return "call-new";
};

console.log("call:", G());
console.log("alias:", alias());

function C() {
  this.v = "ctor-old";
}

C = function () {
  this.v = "ctor-new";
};

console.log("new:", new C().v);
