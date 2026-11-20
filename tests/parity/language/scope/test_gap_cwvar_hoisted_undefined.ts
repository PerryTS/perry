// Closed World: hoisted reads of a module-level var before its declarator
// see undefined, both directly and from a function.
console.log(typeof V, V === undefined, String(V));
function peek(): string {
  return typeof V + ":" + String(V);
}
console.log(peek());
var V: any = (function () {
  var a: any = [];
  for (var i = 1; i <= 3; i++) a.push(i * 10);
  return a;
})();
console.log(typeof V, V === undefined, String(V), V.length);
console.log(peek());
