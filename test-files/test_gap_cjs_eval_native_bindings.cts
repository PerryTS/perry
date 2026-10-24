// Binding adapters retain live native cells while the eval scope owns its vars.
let outerValue = 10;
function run(source) {
  let ownValue = 3;
  eval(source);
  return ownValue;
}
console.log("native", run("ownValue += 4; outerValue += 2"), outerValue);
function shadow(source) {
  eval(source);
  return eval(["outerValue"].join(""));
}
console.log("shadow", shadow("var outerValue = 40"), outerValue);
function retain(source) {
  let value = 1;
  return eval(source);
}
const next = retain("() => ++value");
console.log("retained", next(), next());
function nested(source) {
  eval(source);
  function strictRead(code) {
    "use strict";
    return eval(code);
  }
  function sloppyRead(code) {
    return eval(code);
  }
  return [strictRead("outerValue += 1"), sloppyRead("outerValue += 1")].join(",");
}
console.log("nested", nested("var outerValue = 20"));
console.log("isolated", typeof injected);
const throughWith = require("./fixtures/cjs-eval-with.cjs");
console.log("with", throughWith("outerValue += 1"), outerValue);
module.exports = {};
