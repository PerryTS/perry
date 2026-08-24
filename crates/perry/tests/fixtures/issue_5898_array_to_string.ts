const originalToString = Array.prototype.toString;

(Array.prototype as any).toString = Object.prototype.toString;
console.log(Array.prototype.toString === Object.prototype.toString);
console.log(Array().toString());
console.log(Array(0, 1, 2).toString());
console.log(new Array().toString());
console.log(new Array(0, 1, 2).toString());
console.log(new Array(0).toString());

(Array.prototype as any).toString = function () { return "custom toString"; };
console.log(Array().toString());

(Array.prototype as any).toString = originalToString;
console.log(Array.prototype.toString.call(true));
console.log(Array.prototype.toString.call(false));
console.log(Array.prototype.toString.call({ join() { return "custom join"; } }));
console.log(Array.prototype.toString.call({ join: null }));
