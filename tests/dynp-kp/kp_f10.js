function F(){} const L=[F];
F.x=1; console.log(L[0].x); delete F.x; console.log(L[0].x, L[0].hasOwnProperty("x"));
Object.defineProperty(F,"name",{value:"renamed"}); console.log(L[0].name);
for(let i=0;i<40;i++) F["s"+i]=i;
let s=0; for(let i=0;i<40;i++) s+=L[0]["s"+i]; console.log(s, L[0].s39, L[0].s0, L[0].s20, L[0].call===Function.prototype.call);
F.call=7; console.log(L[0].call); delete F.call; console.log(L[0].call===Function.prototype.call);
