function F(a,b){} class C{} const bf=F.bind(null,1), bc=C.bind(null);
bf.own=5;
const A=[bf,bc];
for(let r=0;r<4;r++){const b=A[r&1]; console.log(r, b.prototype, b.name, b.length, b.own, b.call===Function.prototype.call, b.hasOwnProperty("prototype"))}
