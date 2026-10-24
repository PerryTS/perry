// Non-eval function frames must not become an eval scope prototype.
function outer() {
  const target = { value: 0 };
  function patch(code) { eval(code); }
  patch(["target.value = 7;"].join(""));
  console.log("nested", target.value);
}
outer();
function dynamicOuter() {
  eval(["var injected = 40;"].join(""));
  function middle() {
    function leaf(code) { eval(code); return injected; }
    return leaf(["injected += 2;"].join(""));
  }
  return middle();
}
console.log("ancestor", dynamicOuter());
function strictOuter() {
  "use strict";
  const target = { value: 1 };
  function patch(code) { eval(code); }
  patch(["target.value = 9;"].join(""));
  console.log("strict", target.value);
}
strictOuter();
console.log("isolated", typeof injected);
