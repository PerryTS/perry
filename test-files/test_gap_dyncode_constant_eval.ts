let total = 1;
console.log(eval("total += 41; total"));
console.log(total);
eval("Array.prototype.join = function () { return 'static-patch'; }");
console.log([1, 2].join(","));
