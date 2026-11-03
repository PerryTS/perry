const savedBind = Function.prototype.bind;
const source = process.argv[2] || "Function.prototype.bind = function () { return function () { return 703; }; }";
new Function(source)();
const a = Math.max.bind(null, 2)(3);
const b = Promise.resolve.bind(Promise)(1);
const restore = process.argv[3] || "target.bind = value";
new Function("target", "value", restore)(Function.prototype, savedBind);
console.log(a, b);
