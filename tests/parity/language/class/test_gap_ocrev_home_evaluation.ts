function mk(P){return class extends P{m(){return super.m()}}} class X{m(){return "X"}} class Y{m(){return "Y"}} mk(X); const B=mk(Y); console.log(B.prototype.m.call({}));
