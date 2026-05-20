function func() {}
let called = false;
(func as any).toString = function() { called = true; return "custom"; };
console.log("function object:", func);
console.log("toString called:", called);
