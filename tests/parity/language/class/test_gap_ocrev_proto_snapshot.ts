function F(){} const o=F.prototype; class A extends F{} F.prototype={}; console.log(Object.getPrototypeOf(A.prototype)===o);
