function F(){} const p={q:7}; const A=[F,function(){}];
Object.defineProperty(F,"prototype",{value:p,writable:false});
for(let i=0;i<2;i++){console.log(A[i&2].prototype===p, A[i&2].prototype.q, typeof A[1].prototype)}
try{(function(){"use strict";A[0].prototype=5})()}catch(e){console.log("TE",e instanceof TypeError)}
console.log(A[0].prototype===p);
try{Object.defineProperty(F,"prototype",{get(){return 1}})}catch(e){console.log("TE2",e instanceof TypeError)}
console.log(A[0].prototype===p, Object.getPrototypeOf(new A[0]())===p);
