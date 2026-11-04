const source = process.argv[2] ||
  "Math.max.call = function () { return 901; };" +
  "Array.prototype.slice.call = function () { return 902; };" +
  "Object.prototype.toString.call = function () { return 903; };";
new Function(source)();
const a = Math.max.call(null, 2, 3);
const b = Array.prototype.slice.call([1, 2], 0);
const c = Object.prototype.toString.call({});
delete (Math.max as any).call;
delete (Array.prototype.slice as any).call;
delete (Object.prototype.toString as any).call;
console.log(a, b, c);
