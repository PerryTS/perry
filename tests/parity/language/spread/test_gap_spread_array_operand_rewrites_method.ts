// The method is selected before the arguments for an ARRAY spread too (the dispatch tower, not the
// generator path): an operand that rewrites the prototype method or adds an own method still calls
// the method read first. Parameter receivers, two implementors: the site is a dispatch tower.
class V { set(a, b, c) { return "V:" + a + b + c; } }
class W { set(a, b, c) { return "W:" + a + b + c; } }
const origV = V.prototype.set, origW = W.prototype.set;
function rewrite() {
  V.prototype.set = function (a, b, c) { return "rewrittenV:" + a + b + c; };
  W.prototype.set = function (a, b, c) { return "rewrittenW:" + a + b + c; };
  return [1, 2];
}
function restore() { V.prototype.set = origV; W.prototype.set = origW; }
function viaCall(o) { return o.set(...rewrite(), 3); }
function viaTail(o) { return o.set(...[4, 5], (V.prototype.set = W.prototype.set = function () { return "late"; }, 6)); }
function viaOwn(o) { return o.set(0, ...(o.set = function () { return "own"; }, [7, 8])); }
for (const o of [new V(), new W()]) {
  console.log(viaCall(o), o.set(1, 2, 3)); restore();
  console.log(viaTail(o), o.set(1, 2, 3)); restore();
  console.log(viaOwn(o), o.set(1, 2, 3)); delete o.set;
  console.log(o.set(...[1, 2, 3]));
}
