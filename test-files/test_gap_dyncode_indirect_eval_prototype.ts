const evaluate = eval;
const source = process.argv[2] || "Array.prototype.join = function () { return 'indirect-patch'; }";
evaluate(source);
console.log([1, 2].join(','));
