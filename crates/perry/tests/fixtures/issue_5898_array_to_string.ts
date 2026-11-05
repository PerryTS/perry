try {
  (Array.prototype as any).toString = Object.prototype.toString;
  console.log("no throw");
} catch (e) {
  console.log(e instanceof TypeError);
}
console.log(Array(0, 1, 2).toString());
console.log(new Array().toString());
class Sub extends Array { toString() { return "sub toString"; } }
console.log(new Sub().toString());
console.log(Array.prototype.toString.call(true));
console.log(Array.prototype.toString.call(false));
console.log(Array.prototype.toString.call({ join() { return "custom join"; } }));
console.log(Array.prototype.toString.call({ join: null }));
