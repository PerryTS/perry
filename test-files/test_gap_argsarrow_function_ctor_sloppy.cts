// Function constructor bodies start sloppy even inside strict source.
function make() {
  "use strict";
  return new Function("a", "b", "(() => { arguments[0] = b; })(); return a;");
}
console.log("constructed-arrow", make()(1, 2));
function makeReverse() {
  "use strict";
  return new Function("a", "a = 7; return arguments[0];");
}
console.log("constructed-reverse", makeReverse()(1));
function makeStrict() {
  "use strict";
  return new Function("a", '"use strict"; arguments[0] = 11; return a;');
}
console.log("constructed-strict", makeStrict()(1));
function makeDefault() {
  "use strict";
  return new Function("a=1", "arguments[0] = 13; return a;");
}
console.log("constructed-default", makeDefault()(1));
