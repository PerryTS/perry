class P{static f=function(){return this}} console.log(P.f()===P); class C extends P{} console.log(C.f()===C);
