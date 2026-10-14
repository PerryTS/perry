import vm from 'node:vm';
vm.runInThisContext("Array.prototype.join = function () { return 'vm-patch'; }");
console.log([1, 2].join(","));
