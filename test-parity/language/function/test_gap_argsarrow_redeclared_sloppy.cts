// A var declaration of a parameter must keep its original mapped cell.
function redeclared(a) { var a = 5; return [a, arguments[0]].join(","); }
function noInit(a) { var a; return [a, arguments[0]].join(","); }
function captured(a, b) {
  var a = b;
  const write = () => { arguments[0] = 7; };
  const read = () => a;
  write();
  return [a, read(), arguments[0]].join(",");
}
const expression = function(a) { var a = 11; return [a, arguments[0]].join(","); };
console.log("redeclared", redeclared(1), noInit(3), captured(1, 2), expression(1));
function missing(a) { var a = 13; return [a, arguments[0] === undefined].join(","); }
console.log("redeclared-missing", missing());
function strict(a) { "use strict"; var a = 17; return [a, arguments[0]].join(","); }
console.log("redeclared-strict", strict(1));

const nestedExpression = function(a, b) {
  if (b) { var a = 19; }
  const write = () => { arguments[0] = 23; };
  write();
  return [a, arguments[0]].join(",");
};
const emptyExpression = function(a) { var a; return [a, arguments[0]].join(","); };
console.log("redeclared-expressions", nestedExpression(1, true), emptyExpression(29));
