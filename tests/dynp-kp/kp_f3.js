class K{} const A=[K,Array,Map];
(function(){"use strict"; for(let r=0;r<2;r++) for(let i=0;i<3;i++){const c=A[i]; const p=c.prototype; try{c.prototype=1;console.log("no throw")}catch(e){console.log(i,e instanceof TypeError, A[i].prototype===p)}}})();
for(let i=0;i<3;i++){const d=Object.getOwnPropertyDescriptor(A[i],"prototype"); console.log(i,d.writable,d.configurable)}
