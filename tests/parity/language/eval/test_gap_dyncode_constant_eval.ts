let total = 1;
console.log(eval("total += 41; total"));
console.log(total);
try { eval("Array.prototype.join = function () { return 'static-patch'; }"); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
console.log([1, 2].join(","));
